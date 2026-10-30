//! A budget file cut short, or left empty, must not pass as a healthy budget.

use budget_core::crypto::{xor_line, LineCipher, HEADER_BLOWFISH, HEADER_XOR};
use budget_core::{bgt, BudgetDb, Cal, Payment};
use std::fs;
use std::path::PathBuf;

const TODAY: i32 = 20261030;

fn budget() -> String {
    let mut db = BudgetDb::new(&Cal::from_int(20261001), 12, false);
    for (name, amount) in [("Rent", -1100.0), ("Pay", 2500.0), ("Power", -90.0)] {
        let mut p = Payment::new(20261001);
        p.description = name.into();
        p.amount = amount;
        db.add(p);
    }
    db.to_text(None).unwrap()
}

fn test_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/truncated-file-tests").join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn an_empty_file_or_one_with_no_data_is_refused() {
    for text in ["", "\n", "\r\n\r\n", "\u{feff}", "# just a comment\n", "#db*sea"] {
        let error = bgt::load_str(text, TODAY, None).unwrap_err();
        assert!(error.log.contains("empty"), "{text:?}: {}", error.log);
    }
}

#[test]
fn a_budget_the_app_wrote_with_no_payments_opens_as_complete() {
    let db = BudgetDb::new(&Cal::from_int(20261001), 12, false);
    for password in [None, Some("pw")] {
        let text = db.to_text(password).unwrap();
        let loaded = bgt::load_str(&text, TODAY, password).unwrap_or_else(|l| panic!("{}", l.log));
        assert!(loaded.payments.is_empty() && !loaded.missing_end_marker);
    }
}

#[test]
fn a_complete_budget_is_not_flagged() {
    assert!(!bgt::load_str(&budget(), TODAY, None).unwrap().missing_end_marker);
}

/// At every byte a cut either fails to parse or reports the missing marker, never a healthy budget.
#[test]
fn a_budget_cut_at_any_byte_never_reads_as_complete() {
    let full = budget();
    for cut in 0..full.len() {
        if let Ok(loaded) = bgt::load_str(&full[..cut], TODAY, None) {
            assert!(loaded.missing_end_marker, "cut at {cut}: {:?}", &full[..cut]);
        }
    }
}

#[test]
fn a_line_count_that_disagrees_is_refused() {
    let full = budget();
    let without_rent: String = full.lines().filter(|l| !l.contains("Rent")).collect::<Vec<_>>().join("\n");
    let error = bgt::load_str(&without_rent, TODAY, None).unwrap_err();
    assert!(error.log.contains("mismatched number of read lines"), "{}", error.log);
}

/// An older file with no marker, like the original program's sample, still loads in full and is
/// only reported.
#[test]
fn an_older_file_without_the_marker_still_loads_everything() {
    let text = "v,2\n:\n=,\n-10.0, A, 20260101, 1m, 5, 20261201, 48,  \n";
    let loaded = bgt::load_str(text, TODAY, None).unwrap_or_else(|l| panic!("{}", l.log));
    assert_eq!(loaded.payments.len(), 1);
    assert!(loaded.missing_end_marker);
    assert_eq!(loaded.log, "done.\n1 entries loaded.\n", "the household import compares this text");
}

#[test]
fn old_encrypted_files_with_their_marker_load_unchanged() {
    let plain = budget();
    let lines: Vec<&str> = plain.lines().collect();
    // In these formats the header is line one, so the trailer counts one more.
    let (body, _) = lines.split_at(lines.len() - 1);
    let trailer = format!("## END-OF-DATA; line_no={} ##", lines.len() + 1);
    let all: Vec<&str> = body.iter().copied().chain([trailer.as_str()]).collect();
    let cipher = LineCipher::new("secret").unwrap();
    let blowfish = format!("{HEADER_BLOWFISH}\n{}", all.iter().map(|l| cipher.encrypt_line(l)).collect::<Vec<_>>().join("\n"));
    let xor = format!("{HEADER_XOR}\n{}", all.iter().map(|l| xor_line(l, "secret")).collect::<Vec<_>>().join("\n"));
    for text in [blowfish, xor] {
        let loaded = bgt::load_str(&text, TODAY, Some("secret")).unwrap_or_else(|l| panic!("{}", l.log));
        assert_eq!(loaded.payments.len(), 3);
        assert!(!loaded.missing_end_marker);
    }
}

#[test]
fn a_cut_sealed_file_is_refused() {
    let text = BudgetDb::new(&Cal::from_int(20261001), 12, false).to_text(Some("pw")).unwrap();
    assert!(bgt::load_str(&text[..text.len() / 2], TODAY, Some("pw")).is_err());
}

#[test]
fn backups_are_listed_newest_first_with_their_sizes_and_only_real_backups_count() {
    use std::time::{Duration, SystemTime};
    let dir = test_dir("listing");
    let path = dir.join("home.bgt");
    assert!(bgt::list_backups(&path).is_empty());
    fs::write(&path, b"v0").unwrap();
    for n in ["version one", "version two"] { bgt::save_file(&path, n).unwrap(); }
    // Made by hand, so listed; these look-alikes and a directory are not backups.
    fs::write(dir.join("home.bgt.bak.9"), b"my own copy").unwrap();
    for other in ["home.bgt.bak.old", "other.bgt.bak", "home.bgt.backup"] { fs::write(dir.join(other), b"x").unwrap(); }
    fs::create_dir(dir.join("home.bgt.bak.7")).unwrap();
    let set = |name: &str, ago: u64| fs::File::options().write(true).open(dir.join(name)).unwrap().set_modified(SystemTime::now() - Duration::from_secs(ago)).unwrap();
    set("home.bgt.bak", 300); set("home.bgt.bak.1", 200); set("home.bgt.bak.9", 100);
    let before = fs::read(&path).unwrap();

    let list = bgt::list_backups(&path);

    let names: Vec<_> = list.iter().map(|b| b.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["home.bgt.bak.9", "home.bgt.bak.1", "home.bgt.bak"]);
    assert_eq!(list.iter().map(|b| b.size).collect::<Vec<_>>(), [11, 11, 2]);
    assert!(list.iter().all(|b| b.modified.is_some()));
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::remove_dir_all(dir).unwrap();
}
