//! Off-thread file work for remembered budgets: existence / modified-time checks and reading.
//!
//! A recent file on an unreachable network share makes `metadata()` or `read()` block for ~20 s.
//! These run on worker threads; the UI reads a per-path cache (probes) or polls a [`Task`]
//! (reads) and shows a neutral placeholder until the answer arrives. Each path is probed once.

use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    /// The probe has not answered yet.
    Checking,
    /// Not found, not a file, or unreachable.
    Missing,
    /// A file, with its modification time when the platform reports one.
    Present(Option<SystemTime>),
}

type Probe = Arc<dyn Fn(&Path) -> FileStatus + Send + Sync>;
/// Reads a whole file (`std::fs::read`, or a stand-in in tests).
pub type Reader = Arc<dyn Fn(&Path) -> std::io::Result<Vec<u8>> + Send + Sync>;

/// A job running on a worker thread.
pub struct Task<T> { rx: Receiver<T> }

pub enum TaskState<T> {
    Running,
    Done(T),
    /// The worker stopped without an answer (it could not start, or it panicked).
    Lost,
}

impl<T> Task<T> {
    /// The answer once it has arrived. Never blocks.
    pub fn poll(&self) -> TaskState<T> {
        match self.rx.try_recv() {
            Ok(answer) => TaskState::Done(answer),
            Err(TryRecvError::Empty) => TaskState::Running,
            Err(TryRecvError::Disconnected) => TaskState::Lost,
        }
    }
}

pub struct FileProbes {
    probe: Probe,
    reader: Reader,
    cache: HashMap<PathBuf, FileStatus>,
    tx: Sender<(PathBuf, FileStatus)>,
    rx: Receiver<(PathBuf, FileStatus)>,
    /// Repainted when an answer arrives, so an idle window updates.
    waker: Arc<Mutex<Option<egui::Context>>>,
}

/// The real check: a regular file and its modification time.
pub fn probe_file(path: &Path) -> FileStatus {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => FileStatus::Present(meta.modified().ok()),
        _ => FileStatus::Missing,
    }
}

impl Default for FileProbes {
    fn default() -> Self { FileProbes::with_probe(probe_file) }
}

impl FileProbes {
    pub fn with_probe(probe: impl Fn(&Path) -> FileStatus + Send + Sync + 'static) -> FileProbes {
        let (tx, rx) = channel();
        FileProbes { probe: Arc::new(probe), reader: Arc::new(|path: &Path| std::fs::read(path)), cache: HashMap::new(), tx, rx, waker: Arc::default() }
    }

    /// Reads files with `reader` instead of `std::fs::read`.
    #[cfg(test)]
    pub fn with_reader(mut self, reader: impl Fn(&Path) -> std::io::Result<Vec<u8>> + Send + Sync + 'static) -> FileProbes {
        self.reader = Arc::new(reader);
        self
    }

    /// Remember the context to repaint when a probe answers.
    pub fn wake(&self, ctx: &egui::Context) {
        let mut waker = self.waker.lock().unwrap_or_else(|e| e.into_inner());
        if waker.is_none() { *waker = Some(ctx.clone()); }
    }

    /// Runs `work` on a worker thread with the file reader; the window repaints when it is done.
    pub fn spawn<T: Send + 'static>(&self, work: impl FnOnce(&Reader) -> T + Send + 'static) -> Task<T> {
        let (tx, rx) = channel();
        let (reader, waker) = (self.reader.clone(), self.waker.clone());
        // A thread that cannot start drops `tx`, which the task reports as lost.
        let _ = std::thread::Builder::new().name("file-read".into()).spawn(move || {
            if tx.send(work(&reader)).is_ok() {
                if let Some(ctx) = waker.lock().unwrap_or_else(|e| e.into_inner()).as_ref() { ctx.request_repaint(); }
            }
        });
        Task { rx }
    }

    /// The cached answer for `path`, starting a background probe the first time. Never blocks.
    pub fn status(&mut self, path: &Path) -> FileStatus {
        self.collect();
        if let Some(status) = self.cache.get(path) { return *status; }
        self.cache.insert(path.to_path_buf(), FileStatus::Checking);
        let (probe, tx, waker, owned) = (self.probe.clone(), self.tx.clone(), self.waker.clone(), path.to_path_buf());
        let spawned = std::thread::Builder::new().name("file-probe".into()).spawn(move || {
            let status = probe(&owned);
            if tx.send((owned, status)).is_ok() {
                if let Some(ctx) = waker.lock().unwrap_or_else(|e| e.into_inner()).as_ref() { ctx.request_repaint(); }
            }
        });
        if spawned.is_err() { self.cache.insert(path.to_path_buf(), FileStatus::Missing); }
        self.cache[path]
    }

    /// Like [`Self::status`], but waits up to `timeout` for the answer (startup only).
    pub fn wait(&mut self, path: &Path, timeout: Duration) -> FileStatus {
        let deadline = Instant::now() + timeout;
        loop {
            let status = self.status(path);
            if status != FileStatus::Checking { return status; }
            match self.rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok((done, status)) => { self.cache.insert(done, status); }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return FileStatus::Checking,
            }
        }
    }

    fn collect(&mut self) {
        while let Ok((path, status)) = self.rx.try_recv() { self.cache.insert(path, status); }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    type Release = Arc<(Mutex<bool>, std::sync::Condvar)>;

    /// A probe or a read that blocks until released, counting how often it was started.
    pub(crate) struct Gate { pub calls: Arc<AtomicUsize>, release: Release }

    fn pass(calls: &AtomicUsize, release: &Release) {
        calls.fetch_add(1, Ordering::SeqCst);
        let (open, signal) = &**release;
        let mut released = open.lock().unwrap();
        while !*released { released = signal.wait(released).unwrap(); }
    }

    impl Gate {
        fn new() -> Gate { Gate { calls: Arc::default(), release: Arc::default() } }

        pub(crate) fn probes(answer: FileStatus) -> (Gate, FileProbes) {
            let gate = Gate::new();
            let (calls, release) = (gate.calls.clone(), gate.release.clone());
            let probes = FileProbes::with_probe(move |_| { pass(&calls, &release); answer });
            (gate, probes)
        }

        /// Probes that find every file at once, and reads that wait for the gate.
        pub(crate) fn reads() -> (Gate, FileProbes) {
            let gate = Gate::new();
            let (calls, release) = (gate.calls.clone(), gate.release.clone());
            let probes = FileProbes::with_probe(|_| FileStatus::Present(None))
                .with_reader(move |path| { pass(&calls, &release); std::fs::read(path) });
            (gate, probes)
        }

        pub(crate) fn open(&self) {
            let (open, signal) = &*self.release;
            *open.lock().unwrap() = true;
            signal.notify_all();
        }
    }

    #[test]
    fn a_slow_read_runs_on_a_worker_and_reports_when_done() {
        let (gate, probes) = Gate::reads();
        let task = probes.spawn(|read| read(Path::new(r"\\unreachable\share\Home.bgt")).map_err(|e| e.kind()));
        let started = Instant::now();
        for _ in 0..50 { assert!(matches!(task.poll(), TaskState::Running)); }
        assert!(started.elapsed() < Duration::from_millis(500), "poll() must not wait for the read");
        gate.open();
        let deadline = Instant::now() + Duration::from_secs(10);
        let answer = loop {
            match task.poll() {
                TaskState::Running if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                TaskState::Done(answer) => break answer,
                _ => panic!("the read never answered"),
            }
        };
        assert!(answer.is_err());
        assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
        let lost = probes.spawn(|_| -> u8 { panic!("a failing job") });
        let deadline = Instant::now() + Duration::from_secs(10);
        while matches!(lost.poll(), TaskState::Running) && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(5)); }
        assert!(matches!(lost.poll(), TaskState::Lost), "a job that stops without an answer is reported lost");
    }

    fn settle(probes: &mut FileProbes, path: &Path) -> FileStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = probes.status(path);
            if status != FileStatus::Checking || Instant::now() > deadline { return status; }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_slow_probe_never_blocks_and_each_path_is_probed_once() {
        let (gate, mut probes) = Gate::probes(FileStatus::Missing);
        let path = Path::new(r"\\unreachable\share\Home.bgt");
        let started = Instant::now();
        for _ in 0..50 { assert_eq!(probes.status(path), FileStatus::Checking); }
        assert!(started.elapsed() < Duration::from_millis(500), "status() must not wait for the probe");
        assert_eq!(probes.wait(path, Duration::from_millis(20)), FileStatus::Checking, "wait() gives up at its timeout");
        gate.open();
        assert_eq!(settle(&mut probes, path), FileStatus::Missing);
        assert_eq!(probes.status(path), FileStatus::Missing);
        assert_eq!(gate.calls.load(Ordering::SeqCst), 1, "a path is probed once, not every frame");
    }

    #[test]
    fn the_real_probe_reports_files_with_their_modified_time() {
        let dir = std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/file-probe-tests")).unwrap()
            .join(std::process::id().to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("present.bgt");
        std::fs::write(&file, b"x").unwrap();
        let mut probes = FileProbes::default();
        assert!(matches!(probes.wait(&file, Duration::from_secs(10)), FileStatus::Present(Some(_))));
        assert_eq!(probes.wait(&dir.join("absent.bgt"), Duration::from_secs(10)), FileStatus::Missing);
        assert_eq!(probes.wait(&dir, Duration::from_secs(10)), FileStatus::Missing, "a folder is not a budget");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
