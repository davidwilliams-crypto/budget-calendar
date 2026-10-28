//! File saving preserves the current document and the newest `bgt::MAX_BACKUPS` backups.

use budget_core::bgt;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/file-save-tests");
        fs::create_dir_all(&root).unwrap();
        loop {
            let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("budget-core-save-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test directory: {error}"),
            }
        }
    }

    fn file(&self, name: &str) -> PathBuf { self.0.join(name) }

    fn entries(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0).unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        // These tests create only files and empty directories directly in this private directory.
        if let Ok(entries) = fs::read_dir(&self.0) {
            for entry in entries.flatten() {
                let path = entry.path();
                let _ = if path.is_dir() { fs::remove_dir(path) } else { fs::remove_file(path) };
            }
        }
        let _ = fs::remove_dir(&self.0);
    }
}

#[test]
fn new_file_preserves_legacy_latin1_bytes() {
    let dir = TestDir::new();
    let path = dir.file("new.bgt");

    bgt::save_file(&path, "v,2\n=,caf\u{e9} \u{20ac}").unwrap();

    assert_eq!(fs::read(path).unwrap(), b"v,2\n=,caf\xe9 ?");
    assert_eq!(dir.entries(), ["new.bgt"]);
}

#[test]
fn byte_identical_save_leaves_file_and_existing_backup_untouched() {
    let dir = TestDir::new();
    let path = dir.file("same.bgt");
    fs::write(&path, b"unchanged").unwrap();
    fs::write(dir.file("same.bgt.bak"), b"earlier").unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();

    bgt::save_file(&path, "unchanged").unwrap();

    assert_eq!(fs::read(&path).unwrap(), b"unchanged");
    assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    assert_eq!(fs::read(dir.file("same.bgt.bak")).unwrap(), b"earlier");
    assert_eq!(dir.entries(), ["same.bgt", "same.bgt.bak"]);
}

#[test]
fn replacing_existing_file_keeps_original_bytes_in_backup() {
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"original\r\nbytes\x80").unwrap();

    bgt::save_file(&path, "edited").unwrap();

    assert_eq!(fs::read(path).unwrap(), b"edited");
    assert_eq!(fs::read(dir.file("budget.bgt.bak")).unwrap(), b"original\r\nbytes\x80");
    assert_eq!(dir.entries(), ["budget.bgt", "budget.bgt.bak"]);
}

#[test]
fn repeated_changes_keep_all_prior_versions() {
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"version one").unwrap();

    bgt::save_file(&path, "version two").unwrap();
    bgt::save_file(&path, "version three").unwrap();

    assert_eq!(fs::read(path).unwrap(), b"version three");
    assert_eq!(fs::read(dir.file("budget.bgt.bak")).unwrap(), b"version one");
    assert_eq!(fs::read(dir.file("budget.bgt.bak.1")).unwrap(), b"version two");
    assert_eq!(dir.entries(), ["budget.bgt", "budget.bgt.bak", "budget.bgt.bak.1"]);
}

#[test]
fn only_the_newest_backups_are_kept_and_numbering_continues() {
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"v0").unwrap();
    // Look-alikes that are not this budget's backups must survive pruning.
    for other in ["budget.bgt.bak.old", "budget.bgt.bak.1a", "other.bgt.bak", "budget.bgt.backup"] {
        fs::write(dir.file(other), b"keep").unwrap();
    }

    for n in 1..=bgt::MAX_BACKUPS + 2 {
        bgt::save_file(&path, &format!("v{n}")).unwrap();
    }

    // Twelve saves made backups .bak (v0) .. .bak.11 (v11); the two oldest were pruned.
    assert!(!dir.file("budget.bgt.bak").exists());
    assert!(!dir.file("budget.bgt.bak.1").exists());
    for index in 2..=11 {
        assert_eq!(fs::read(dir.file(&format!("budget.bgt.bak.{index}"))).unwrap(), format!("v{index}").as_bytes());
    }
    bgt::save_file(&path, "v13").unwrap();
    assert_eq!(fs::read(dir.file("budget.bgt.bak.12")).unwrap(), b"v12", "a gap left by pruning is not reused");
    assert!(!dir.file("budget.bgt.bak.2").exists());
    for other in ["budget.bgt.bak.old", "budget.bgt.bak.1a", "other.bgt.bak", "budget.bgt.backup"] {
        assert_eq!(fs::read(dir.file(other)).unwrap(), b"keep");
    }
}

#[test]
fn pruning_never_removes_a_directory_named_like_a_backup() {
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"v0").unwrap();
    fs::create_dir(dir.file("budget.bgt.bak")).unwrap();

    for n in 1..=bgt::MAX_BACKUPS + 1 {
        bgt::save_file(&path, &format!("v{n}")).unwrap();
    }

    assert!(dir.file("budget.bgt.bak").is_dir());
    assert!(!dir.file("budget.bgt.bak.1").exists());
    assert_eq!(fs::read(dir.file("budget.bgt.bak.11")).unwrap(), b"v10");
}

#[test]
fn existing_backup_files_and_directories_are_never_overwritten() {
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"original").unwrap();
    fs::write(dir.file("budget.bgt.bak"), b"older backup").unwrap();
    fs::create_dir(dir.file("budget.bgt.bak.1")).unwrap();

    bgt::save_file(&path, "edited").unwrap();

    assert_eq!(fs::read(path).unwrap(), b"edited");
    assert_eq!(fs::read(dir.file("budget.bgt.bak")).unwrap(), b"older backup");
    assert!(dir.file("budget.bgt.bak.1").is_dir());
    assert_eq!(fs::read(dir.file("budget.bgt.bak.2")).unwrap(), b"original");
    assert_eq!(dir.entries(), ["budget.bgt", "budget.bgt.bak", "budget.bgt.bak.1", "budget.bgt.bak.2"]);
}

#[test]
fn missing_destination_directory_does_not_change_existing_files() {
    let dir = TestDir::new();
    let original = dir.file("budget.bgt");
    fs::write(&original, b"original").unwrap();
    fs::write(dir.file("budget.bgt.bak"), b"older backup").unwrap();
    let content = String::from("unsaved edits");

    assert!(bgt::save_file(&dir.file("missing").join("budget.bgt"), &content).is_err());

    assert_eq!(content, "unsaved edits");
    assert_eq!(fs::read(original).unwrap(), b"original");
    assert_eq!(fs::read(dir.file("budget.bgt.bak")).unwrap(), b"older backup");
    assert_eq!(dir.entries(), ["budget.bgt", "budget.bgt.bak"]);
}

#[test]
fn directory_destination_is_preserved_on_failure() {
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::create_dir(&path).unwrap();

    assert!(bgt::save_file(&path, "edits").is_err());

    assert!(path.is_dir());
    assert_eq!(dir.entries(), ["budget.bgt"]);
}

#[cfg(windows)]
#[test]
fn failed_replacement_keeps_original_and_cleans_own_temp_and_backup() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"original").unwrap();
    // Allow reading the original for its backup, but deny replacement/deletion.
    let held = fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();

    let err = bgt::save_file(&path, "edited").unwrap_err();
    assert!(err.contains("read-only or open in another program"), "{err}");

    assert_eq!(fs::read(&path).unwrap(), b"original");
    // the failed attempt must not leave its backup behind to pile up on retries
    assert_eq!(dir.entries(), ["budget.bgt"]);
    drop(held);
}
