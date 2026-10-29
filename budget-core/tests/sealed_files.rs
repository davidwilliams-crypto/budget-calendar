//! Generated standalone-file fixtures only. No user files are read.
use budget_core::{bgt, crypto::LineCipher, Payment};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use std::sync::OnceLock;

const PASSWORD: &str = "a deliberately long password beyond the old fifty-six byte boundary: café 🔐 終";
const HEADER: &str = "#db*sealed";
const PLAIN: &str = "v,2\n:\n=,private notes\n-10.0, Rent, 20260101, 1m, 1, 20260101, 48,  \n## END-OF-DATA; line_no=5 ##";

fn sealed() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let l = bgt::load_str(PLAIN, 20260101, None).unwrap();
        bgt::save_str(&l.payments, l.adjustments.as_ref().unwrap(), &l.notes, Some(PASSWORD)).unwrap()
    })
}

fn envelope(text: &str) -> Value {
    assert!(text.starts_with(&format!("{HEADER}\n")), "password saves must use an authenticated envelope");
    serde_json::from_str(text.split_once('\n').unwrap().1).unwrap()
}

fn reject(text: &str, password: Option<&str>) {
    let error = bgt::load_str(text, 20260101, password).unwrap_err();
    assert!(error.payments.is_empty(), "unauthenticated payments escaped");
    assert!(error.adjustments.is_none(), "unauthenticated adjustments escaped");
    assert!(error.notes.is_empty());
    assert!(!error.log.contains("Rent"));
    assert!(!error.log.contains("private notes"));
}

#[test]
fn password_saves_are_randomized_and_preserve_full_unicode_passwords() {
    let a = sealed();
    let meta = envelope(a);
    let l = bgt::load_str(a, 20260101, Some(PASSWORD)).unwrap();
    assert_eq!(l.payments[0].description, "Rent");
    assert_eq!(l.notes, "private notes");
    let b = bgt::save_str(&l.payments, l.adjustments.as_ref().unwrap(), &l.notes, Some(PASSWORD)).unwrap();
    let other = envelope(&b);
    assert_ne!(meta["salt"], other["salt"]);
    assert_ne!(meta["nonce"], other["nonce"]);
    assert_ne!(meta["ciphertext"], other["ciphertext"]);
    assert!(bgt::is_encrypted_text(a));
    reject(a, None);
    reject(a, Some(&PASSWORD[..56]));
    reject(a, Some("wrong password"));
}

#[test]
fn authentication_rejects_body_tag_salt_and_nonce_changes_before_parsing() {
    let meta = envelope(sealed());
    for key in ["ciphertext", "salt", "nonce"] {
        let mut changed = meta.clone();
        let mut bytes = STANDARD.decode(changed[key].as_str().unwrap()).unwrap();
        bytes[0] ^= 1;
        changed[key] = Value::String(STANDARD.encode(bytes));
        reject(&format!("{HEADER}\n{changed}"), Some(PASSWORD));
    }
    let mut changed = meta.clone();
    let mut bytes = STANDARD.decode(changed["ciphertext"].as_str().unwrap()).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    changed["ciphertext"] = Value::String(STANDARD.encode(bytes));
    reject(&format!("{HEADER}\n{changed}"), Some(PASSWORD));
    reject(&sealed()[..sealed().len() - 1], Some(PASSWORD));
    let pieces: Vec<_> = sealed().split('\n').collect();
    reject(&format!("{}\n{}", pieces[1], pieces[0]), Some(PASSWORD));
    reject(&sealed().replacen(HEADER, "#db*sealed-unsupported", 1), Some(PASSWORD));
    let mut bytes = STANDARD.decode(meta["ciphertext"].as_str().unwrap()).unwrap();
    bytes.rotate_left(16);
    let mut reordered = meta;
    reordered["ciphertext"] = Value::String(STANDARD.encode(bytes));
    reject(&format!("{HEADER}\n{reordered}"), Some(PASSWORD));
}

#[test]
fn malformed_and_expensive_envelopes_fail_before_deriving_a_key() {
    let meta = envelope(sealed());
    for (key, value) in [
        ("version", serde_json::json!(999)),
        ("cipher", serde_json::json!("AES-128-GCM")),
        ("kdf", serde_json::json!("argon2d")),
        ("kdfVersion", serde_json::json!(16)),
        ("memoryKiB", serde_json::json!(4294967295u32)),
        ("iterations", serde_json::json!(0)),
        ("parallelism", serde_json::json!(1000)),
        ("salt", serde_json::json!("AA==")),
        ("nonce", serde_json::json!("not-base64!!!")),
        ("ciphertext", serde_json::json!("AA==")),
        ("unknown", serde_json::json!(true)),
    ] {
        let mut changed = meta.clone();
        changed[key] = value;
        reject(&format!("{HEADER}\n{changed}"), Some(PASSWORD));
    }
    let body = sealed().split_once('\n').unwrap().1;
    reject(&format!("{HEADER}\n{{\"version\":1,{}", &body[1..]), Some(PASSWORD));
    reject(&format!("{HEADER}\n{body}\n{body}"), Some(PASSWORD));
    reject(&format!("{HEADER}\n{}", " ".repeat(90 * 1024 * 1024)), Some(PASSWORD));
    reject(sealed(), Some(&"x".repeat(1025 * 1024)));
    assert!(bgt::save_str(&[], &Payment::new(20260101), "", Some("")).is_err());
    assert!(bgt::save_str(&[], &Payment::new(20260101), "", Some(&"x".repeat(1025 * 1024))).is_err());
}

#[test]
fn plaintext_serialization_and_legacy_read_compatibility_are_preserved() {
    let l = bgt::load_str(PLAIN, 20260101, None).unwrap();
    assert_eq!(bgt::save_str(&l.payments, l.adjustments.as_ref().unwrap(), &l.notes, None).unwrap(), PLAIN);
    let legacy_plain = PLAIN.replace("line_no=5", "line_no=6");
    for password in ["x", "ab", "abc", "café 🔐"] {
        for cipher in [LineCipher::new(password).unwrap(), LineCipher::new_legacy(password).unwrap()] {
            let legacy = format!("#db*encrypted\r{}", legacy_plain.lines().map(|l| cipher.encrypt_line(l)).collect::<Vec<_>>().join("\r"));
            let loaded = bgt::load_str(&format!("\u{feff}{legacy}"), 20260101, Some(password)).unwrap();
            assert_eq!(loaded.payments[0].description, "Rent");
        }
    }
}
