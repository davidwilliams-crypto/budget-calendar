//! Off-thread existence / modified-time checks for remembered budgets.
//!
//! A recent file on an unreachable network share makes `metadata()` block for ~20 s. These probes
//! run on worker threads; the UI reads a per-path cache and shows a neutral placeholder until the
//! answer arrives. Each path is probed once.

use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
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

pub struct FileProbes {
    probe: Probe,
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
        FileProbes { probe: Arc::new(probe), cache: HashMap::new(), tx, rx, waker: Arc::default() }
    }

    /// Remember the context to repaint when a probe answers.
    pub fn wake(&self, ctx: &egui::Context) {
        let mut waker = self.waker.lock().unwrap_or_else(|e| e.into_inner());
        if waker.is_none() { *waker = Some(ctx.clone()); }
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

    /// A probe that blocks until released, counting how often it was started.
    pub(crate) struct Gate { pub calls: Arc<AtomicUsize>, release: Arc<(Mutex<bool>, std::sync::Condvar)> }

    impl Gate {
        pub(crate) fn probes(answer: FileStatus) -> (Gate, FileProbes) {
            let gate = Gate { calls: Arc::default(), release: Arc::default() };
            let (calls, release) = (gate.calls.clone(), gate.release.clone());
            let probes = FileProbes::with_probe(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                let (open, signal) = &*release;
                let mut released = open.lock().unwrap();
                while !*released { released = signal.wait(released).unwrap(); }
                answer
            });
            (gate, probes)
        }

        pub(crate) fn open(&self) {
            let (open, signal) = &*self.release;
            *open.lock().unwrap() = true;
            signal.notify_all();
        }
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
