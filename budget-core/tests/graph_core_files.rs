//! Saving a budget or the settings must keep what the user set up around the file: who may read
//! it, and the file a link points to.

use budget_core::bgt;
use budget_core::config::BudgetConfig;
use std::fs;
use std::path::PathBuf;

fn test_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/graph-core-files-tests").join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A budget readable only by its owner must not become readable by others when it is saved, and
/// neither may the backup of its previous contents.
#[cfg(unix)]
#[test]
fn saving_keeps_the_file_permissions_of_the_budget_and_settings() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = test_dir("permissions");

    let budget = dir.join("private.bgt");
    fs::write(&budget, "v,2\nold").unwrap();
    fs::set_permissions(&budget, fs::Permissions::from_mode(0o600)).unwrap();
    bgt::save_file(&budget, "v,2\nnew").unwrap();
    assert_eq!(fs::read_to_string(&budget).unwrap(), "v,2\nnew");
    assert_eq!(fs::read_to_string(dir.join("private.bgt.bak")).unwrap(), "v,2\nold");
    assert_eq!(format!("{:o}", mode(&budget)), "600", "saved budget");
    assert_eq!(format!("{:o}", mode(&dir.join("private.bgt.bak"))), "600", "backup of the budget");

    let rc = dir.join(".budgetrc");
    fs::write(&rc, "autosave=false\n").unwrap();
    fs::set_permissions(&rc, fs::Permissions::from_mode(0o600)).unwrap();
    BudgetConfig::default().save(&rc).unwrap();
    assert_eq!(format!("{:o}", mode(&rc)), "600", "saved settings");

    fs::remove_dir_all(&dir).unwrap();
}

/// Saving through a symbolic link writes the file it points to; it must not replace the link with
/// a separate copy and leave the linked file behind with the old contents.
#[cfg(unix)]
#[test]
fn saving_through_a_symbolic_link_updates_the_linked_file() {
    let dir = test_dir("symlink");
    let shared = dir.join("shared");
    fs::create_dir_all(&shared).unwrap();

    let real = shared.join("home.bgt");
    let link = dir.join("home.bgt");
    fs::write(&real, "v,2\nold").unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    bgt::save_file(&link, "v,2\nnew").unwrap();
    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link was replaced by a regular file");
    assert_eq!(fs::read_to_string(&real).unwrap(), "v,2\nnew", "the linked budget was not updated");
    assert_eq!(fs::read_to_string(shared.join("home.bgt.bak")).unwrap(), "v,2\nold");

    let real_rc = shared.join("budgetrc");
    let link_rc = dir.join(".budgetrc");
    fs::write(&real_rc, "autosave=false\n").unwrap();
    std::os::unix::fs::symlink(&real_rc, &link_rc).unwrap();
    BudgetConfig::default().save(&link_rc).unwrap();
    assert!(fs::symlink_metadata(&link_rc).unwrap().file_type().is_symlink(), "the settings link was replaced by a regular file");
    assert!(BudgetConfig::load(&real_rc).autosave, "the linked settings were not updated");

    fs::remove_dir_all(&dir).unwrap();
}
