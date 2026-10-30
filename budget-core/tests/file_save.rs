//! File saving preserves the current document, the newest `bgt::MAX_BACKUPS` backups and the first
//! backup of each of the last `bgt::BACKUP_DAYS` days.

use budget_core::config::BudgetConfig;
use budget_core::{bgt, BudgetDb, Cal};
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
    assert_eq!(dir.entries(), ["budget.bgt", "budget.bgt.bak", "budget.bgt.baklog"]);
}

#[test]
fn explicit_save_migrates_legacy_encryption_and_backs_up_exact_previous_bytes() {
    let dir = TestDir::new();
    let path = dir.file("generated-legacy.bgt");
    let plain = "v,2\n:\n=,legacy fixture\n## END-OF-DATA; line_no=5 ##";
    let cipher = budget_core::crypto::LineCipher::new("ab").unwrap();
    let legacy = format!("\u{feff}#db*encrypted\r\n{}", plain.lines().map(|l| cipher.encrypt_line(l)).collect::<Vec<_>>().join("\r\n"));
    let old = [b"\xef\xbb\xbf".as_slice(), &bgt::latin1_encode(legacy.trim_start_matches('\u{feff}'))].concat();
    fs::write(&path, &old).unwrap();
    let loaded = bgt::load_file(&path, 20260101, Some("ab")).unwrap();
    assert_eq!(fs::read(&path).unwrap(), old, "opening must not migrate the file");
    assert_eq!(dir.entries(), ["generated-legacy.bgt"]);
    let new = bgt::save_str(&loaded.payments, loaded.adjustments.as_ref().unwrap(), &loaded.notes, Some("ab")).unwrap();
    bgt::save_file(&path, &new).unwrap();
    assert!(new.starts_with("#db*sealed\n"));
    assert_eq!(fs::read(dir.file("generated-legacy.bgt.bak")).unwrap(), old);
    assert_eq!(fs::read(&path).unwrap(), new.as_bytes());
    assert_eq!(bgt::load_file(&path, 20260101, Some("ab")).unwrap().notes, "legacy fixture");
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
    assert_eq!(dir.entries(), ["budget.bgt", "budget.bgt.bak", "budget.bgt.bak.1", "budget.bgt.baklog"]);
}

/// Windows names ignore case, so saving through another spelling of the same file continues the
/// same backup numbering and keeps the same cap.
#[cfg(windows)]
#[test]
fn backups_made_through_another_spelling_share_one_numbering_and_cap() {
    let dir = TestDir::new();
    fs::write(dir.file("Home.bgt"), b"v0").unwrap();
    for n in 1..=8 { bgt::save_file(&dir.file("Home.bgt"), &format!("v{n}")).unwrap(); }
    for n in 9..=13 { bgt::save_file(&dir.file("HOME.BGT"), &format!("v{n}")).unwrap(); }

    // the log lists the backups, but is not one of them
    let backups: Vec<String> = dir.entries().into_iter().filter(|e| e.to_lowercase().contains(".bgt.bak") && !e.to_lowercase().ends_with(".baklog")).collect();
    assert_eq!(backups.len(), bgt::MAX_BACKUPS, "{backups:?}");
    // v0 (the first of today) and v4..v12 survive whichever spelling made them; v1..v3 were pruned.
    assert_eq!(fs::read(dir.file("home.bgt.bak")).unwrap(), b"v0");
    for index in 4..=12 {
        assert_eq!(fs::read(dir.file(&format!("home.bgt.bak.{index}"))).unwrap(), format!("v{index}").as_bytes());
    }
    assert_eq!(fs::read(dir.file("home.bgt")).unwrap(), b"v13");
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

    // Twelve saves made backups .bak (v0) .. .bak.11 (v11); the first of today stays and the two
    // oldest after it were pruned.
    assert_eq!(fs::read(dir.file("budget.bgt.bak")).unwrap(), b"v0");
    assert!(!dir.file("budget.bgt.bak.1").exists());
    assert!(!dir.file("budget.bgt.bak.2").exists());
    for index in 3..=11 {
        assert_eq!(fs::read(dir.file(&format!("budget.bgt.bak.{index}"))).unwrap(), format!("v{index}").as_bytes());
    }
    bgt::save_file(&path, "v13").unwrap();
    assert_eq!(fs::read(dir.file("budget.bgt.bak.12")).unwrap(), b"v12", "a gap left by pruning is not reused");
    assert!(!dir.file("budget.bgt.bak.3").exists());
    for other in ["budget.bgt.bak.old", "budget.bgt.bak.1a", "other.bgt.bak", "budget.bgt.backup"] {
        assert_eq!(fs::read(dir.file(other)).unwrap(), b"keep");
    }
}

/// Twelve quick saves in a row: the version from before them is the one worth having.
#[test]
fn quick_saves_never_erase_the_version_from_before_them() {
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"last good version").unwrap();

    for n in 1..=bgt::MAX_BACKUPS + 2 {
        bgt::save_file(&path, &format!("v{n}")).unwrap();
    }

    assert_eq!(fs::read(dir.file("budget.bgt.bak")).unwrap(), b"last good version");
    assert_eq!(fs::read_to_string(dir.file("budget.bgt.baklog")).unwrap().lines().count(), bgt::MAX_BACKUPS);
}

/// Backups dated one to five days ago, each the first of its day, outlast any number of saves today;
/// a second backup from one of those days goes under the count limit.
#[test]
fn the_first_backup_of_each_recent_day_outlasts_the_count_limit() {
    use std::time::{Duration, SystemTime};
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"v0").unwrap();
    bgt::save_file(&path, "day five").unwrap();
    bgt::save_file(&path, "day five again").unwrap();
    for day in ["day four", "day three", "day two", "day one"] { bgt::save_file(&path, day).unwrap(); }
    // .bak v0 and .bak.1 are five days old; .bak.2 .. .bak.5 are four to one days old.
    let days_ago = |n: u64| SystemTime::now() - Duration::from_secs(n * 86_400);
    let age = [("budget.bgt.bak", 5), ("budget.bgt.bak.1", 5), ("budget.bgt.bak.2", 4), ("budget.bgt.bak.3", 3), ("budget.bgt.bak.4", 2), ("budget.bgt.bak.5", 1)];
    for (name, days) in age {
        fs::File::options().write(true).open(dir.file(name)).unwrap().set_modified(days_ago(days)).unwrap();
    }

    for n in 1..=bgt::MAX_BACKUPS * 2 { bgt::save_file(&path, &format!("today {n}")).unwrap(); }

    for name in ["budget.bgt.bak", "budget.bgt.bak.2", "budget.bgt.bak.3", "budget.bgt.bak.4", "budget.bgt.bak.5"] {
        assert!(dir.file(name).exists(), "{name}");
    }
    assert!(!dir.file("budget.bgt.bak.1").exists(), "a second backup of one day is not protected");
    let log = fs::read_to_string(dir.file("budget.bgt.baklog")).unwrap();
    assert_eq!(log.lines().count(), bgt::MAX_BACKUPS);
}

/// Older than the protected days, the count limit alone applies: the app's folder does not grow forever.
#[test]
fn a_backup_older_than_the_protected_days_goes_under_the_count_limit() {
    use std::time::{Duration, SystemTime};
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"v0").unwrap();
    bgt::save_file(&path, "v1").unwrap();
    let old = SystemTime::now() - Duration::from_secs((bgt::BACKUP_DAYS as u64 + 2) * 86_400);
    fs::File::options().write(true).open(dir.file("budget.bgt.bak")).unwrap().set_modified(old).unwrap();

    for n in 2..=bgt::MAX_BACKUPS + 1 { bgt::save_file(&path, &format!("v{n}")).unwrap(); }

    assert!(!dir.file("budget.bgt.bak").exists());
}

#[test]
fn budgetrc_save_replaces_the_settings_without_leftover_files() {
    let dir = TestDir::new();
    let rc = dir.file(".budgetrc");
    let mut config = BudgetConfig { currency_prefix: "EUR".into(), ..Default::default() };
    config.save(&rc).unwrap();
    config.currency_prefix = "CHF".into();
    config.save(&rc).unwrap();

    assert_eq!(BudgetConfig::load(&rc).currency_prefix, "CHF");
    assert_eq!(dir.entries(), [".budgetrc"]);
}

#[cfg(windows)]
#[test]
fn failed_budgetrc_save_keeps_the_previous_settings_and_leaves_no_backup() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = TestDir::new();
    let rc = dir.file(".budgetrc");
    BudgetConfig { currency_prefix: "EUR".into(), ..Default::default() }.save(&rc).unwrap();
    // Readable by others, but not writable or replaceable while held.
    let held = fs::OpenOptions::new().read(true).share_mode(1).open(&rc).unwrap();

    assert!(BudgetConfig { currency_prefix: "CHF".into(), ..Default::default() }.save(&rc).is_err());

    drop(held);
    assert_eq!(BudgetConfig::load(&rc).currency_prefix, "EUR");
    assert_eq!(dir.entries(), [".budgetrc"]);
}

#[test]
fn refused_save_of_a_non_finite_amount_leaves_the_file_and_backups_alone() {
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"original").unwrap();
    fs::write(dir.file("budget.bgt.bak"), b"older backup").unwrap();
    let text = "v,2\n:\n=,\n-10.0, A, 20260101, 1m, 5, 20261201, 48,  \n## END-OF-DATA; line_no=5 ##";
    let mut db = BudgetDb::from_loaded(bgt::load_str(text, 20260101, None).unwrap_or_else(|l| panic!("{}", l.log)), &Cal::from_int(20260101), 12, false);
    db.payments[0].amount = f64::NAN;

    assert!(db.save(&path, None).is_err());

    assert_eq!(fs::read(&path).unwrap(), b"original");
    assert_eq!(fs::read(dir.file("budget.bgt.bak")).unwrap(), b"older backup");
    assert_eq!(dir.entries(), ["budget.bgt", "budget.bgt.bak"]);
}

#[test]
fn a_backup_made_by_hand_is_never_pruned() {
    let dir = TestDir::new();
    let path = dir.file("home.bgt");
    fs::write(&path, b"v0").unwrap();
    fs::write(dir.file("home.bgt.bak"), b"my own copy").unwrap();
    for other in ["home.bgt.bak.old", "home.bgt.bak.1a", "home.bgt.backup", "other.bgt.bak"] {
        fs::write(dir.file(other), b"keep").unwrap();
    }

    for n in 1..=bgt::MAX_BACKUPS + 2 {
        bgt::save_file(&path, &format!("v{n}")).unwrap();
    }

    assert_eq!(fs::read(dir.file("home.bgt.bak")).unwrap(), b"my own copy");
    // the app's own backups are .bak.1 .. .bak.12 (v0 .. v11); the first of today and the newest
    // nine are kept
    assert_eq!(fs::read(dir.file("home.bgt.bak.1")).unwrap(), b"v0");
    assert!(!dir.file("home.bgt.bak.2").exists());
    assert!(!dir.file("home.bgt.bak.3").exists());
    for index in 4..=12 {
        assert_eq!(fs::read(dir.file(&format!("home.bgt.bak.{index}"))).unwrap(), format!("v{}", index - 1).as_bytes());
    }
    for other in ["home.bgt.bak.old", "home.bgt.bak.1a", "home.bgt.backup", "other.bgt.bak"] {
        assert_eq!(fs::read(dir.file(other)).unwrap(), b"keep");
    }
}

#[test]
fn the_backup_log_names_exactly_the_backups_this_app_kept() {
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"v0").unwrap();

    for n in 1..=bgt::MAX_BACKUPS + 2 {
        bgt::save_file(&path, &format!("v{n}")).unwrap();
    }

    let log = fs::read_to_string(dir.file("budget.bgt.baklog")).unwrap();
    let want: Vec<String> = std::iter::once("budget.bgt.bak".to_string()).chain((3..=11).map(|index| format!("budget.bgt.bak.{index}"))).collect();
    assert_eq!(log.lines().collect::<Vec<_>>(), want);
    assert_eq!(log.lines().count(), bgt::MAX_BACKUPS);
}

#[test]
fn a_file_named_like_the_log_is_never_overwritten() {
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"v0").unwrap();
    fs::write(dir.file("budget.bgt.baklog"), b"my notes, not a log").unwrap();

    for n in 1..=3 {
        bgt::save_file(&path, &format!("v{n}")).unwrap();
    }

    assert_eq!(fs::read(dir.file("budget.bgt.baklog")).unwrap(), b"my notes, not a log");
    // without a log of its own the app removes nothing, so all three backups remain
    assert_eq!(fs::read(dir.file("budget.bgt.bak")).unwrap(), b"v0");
    assert_eq!(fs::read(dir.file("budget.bgt.bak.1")).unwrap(), b"v1");
    assert_eq!(fs::read(dir.file("budget.bgt.bak.2")).unwrap(), b"v2");
}

#[test]
fn a_backup_index_at_the_top_of_the_range_is_an_error_not_a_panic() {
    let dir = TestDir::new();
    let path = dir.file("budget.bgt");
    fs::write(&path, b"original").unwrap();
    fs::write(dir.file(&format!("budget.bgt.bak.{}", u64::MAX)), b"absurd").unwrap();

    assert!(bgt::save_file(&path, "edited").is_err());

    assert_eq!(fs::read(&path).unwrap(), b"original");
    assert_eq!(dir.entries().len(), 2);
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
    assert_eq!(fs::read(dir.file("budget.bgt.bak.1")).unwrap(), b"v0", "the first of today");
    assert!(!dir.file("budget.bgt.bak.2").exists());
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
    assert_eq!(dir.entries(), ["budget.bgt", "budget.bgt.bak", "budget.bgt.bak.1", "budget.bgt.bak.2", "budget.bgt.baklog"]);
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
