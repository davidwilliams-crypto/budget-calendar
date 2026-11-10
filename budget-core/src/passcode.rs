//! Household passcode: a memorable passcode that carries the random 256-bit household key
//! to a new device. The passcode never replaces the key; it wraps it, and the wrap record
//! ("slot") is stored with the household on the service, which sees only ciphertext.
//!
//! `ke = Argon2id(passcode, salt)` (RFC 9106 second recommended profile, identical to the
//! standalone sealed-file format so one cost table governs every user-chosen secret), then
//! `wrappedKey = AES-256-GCM(ke, household key, aad = "BudgetCalendar:passcode:1:{id}")`.
//! The GCM tag is the wrong-passcode test; no verifier blob is needed.

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::{Aead, Payload}};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const SLOT_VERSION: u32 = 1;
pub const KDF_NAME: &str = "argon2id";
pub const KDF_VERSION: u32 = 19;
pub const MEMORY_KI_B: u32 = 65536;
pub const ITERATIONS: u32 = 3;
pub const PARALLELISM: u32 = 4;
pub const PASSCODE_MIN_CHARS: usize = 8;
pub const PASSCODE_MAX_BYTES: usize = 1024;
const AUTH_ERROR: &str = "Unable to unlock this household: incorrect passcode, or the household key has changed.";

/// Common passcodes rejected at set time. The service never sees the passcode, so this check
/// runs wherever the slot is created; Argon2id remains the real defence.
const OBVIOUS: &[&str] = &[
    "00000000", "11111111", "12121212", "123123123", "12341234", "12345123", "12345678", "123456789",
    "1234567890", "20242024", "abcdefg1", "baseball", "basketball", "dragon123", "family123",
    "football", "budget", "budget123", "iloveyou", "letmein123", "michael123", "mustang123", "passcode1", "passcode123",
    "password1", "password123", "princess1", "qwerty123", "sunshine1", "trustno1", "welcome1", "whatever1",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PasscodeSlot {
    pub version: u32,
    pub kdf: String,
    pub kdf_version: u32,
    pub memory_ki_b: u32,
    pub iterations: u32,
    pub parallelism: u32,
    /// 16 bytes, standard base64.
    pub salt: String,
    /// 12 bytes, standard base64.
    pub nonce: String,
    /// 48 bytes (32-byte household key + 16-byte GCM tag), standard base64.
    pub wrapped_key: String,
}

/// Fresh host entropy for the engine's passcode-wrap action. The raw WASM ABI has no OS
/// imports, so the official browser wrapper supplies (and replaces) caller entropy here, as it
/// does for password exports. The fixed array types reject any other length.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WrapEntropy {
    pub salt: [u8; 16],
    pub nonce: [u8; 12],
}

fn check_budget_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 200 || id.bytes().any(|b| !(b.is_ascii_alphanumeric() || b"-".contains(&b))) {
        return Err("Invalid household identity.".into());
    }
    Ok(())
}

/// Public policy gate: the same rule `wrap_with_entropy` enforces, checked before any work.
pub fn check_passcode(passcode: &str) -> Result<(), String> {
    if passcode.chars().count() < PASSCODE_MIN_CHARS {
        return Err(format!("The household passcode needs at least {PASSCODE_MIN_CHARS} characters. Use three or more unrelated words."));
    }
    if passcode.len() > PASSCODE_MAX_BYTES {
        return Err("The household passcode exceeds the supported length.".into());
    }
    if OBVIOUS.iter().any(|bad| passcode.trim().eq_ignore_ascii_case(bad)) {
        return Err("That passcode is too common to protect a budget. Use three or more unrelated words.".into());
    }
    if passcode.trim_start().starts_with("BC1-") {
        return Err("Enter a passcode you make up, not the household's recovery key.".into());
    }
    Ok(())
}

fn derive(passcode: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, String> {
    // Identical profile to `sealed::key`: one cost table, and slots stay attackable only at
    // 64 MiB x 3 passes. Inputs cannot choose cheaper or costlier work.
    let params = Params::new(MEMORY_KI_B, ITERATIONS, PARALLELISM, Some(32)).map_err(|_| AUTH_ERROR)?;
    let mut memory = Vec::new();
    memory.try_reserve_exact(params.block_count()).map_err(|_| "Not enough memory to derive the household passcode key.".to_string())?;
    memory.resize(params.block_count(), argon2::Block::default());
    let mut memory = Zeroizing::new(memory);
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into_with_memory(passcode.as_bytes(), salt, key.as_mut(), memory.as_mut_slice())
        .map_err(|_| "Unable to derive the household passcode key.".to_string())?;
    Ok(key)
}

fn aad(budget_id: &str) -> Result<Vec<u8>, String> {
    check_budget_id(budget_id)?;
    Ok(format!("BudgetCalendar:passcode:{SLOT_VERSION}:{budget_id}").into_bytes())
}

/// Wraps the household key with the passcode. The salt and nonce come from the caller's CSPRNG;
/// `wrap_with_os_entropy` supplies them natively.
pub fn wrap_with_entropy(household_key: &[u8; 32], passcode: &str, budget_id: &str, salt: [u8; 16], nonce: [u8; 12]) -> Result<PasscodeSlot, String> {
    check_passcode(passcode)?;
    let ke = derive(passcode, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(ke.as_ref()).map_err(|_| AUTH_ERROR)?;
    let wrapped = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: household_key, aad: &aad(budget_id)? })
        .map_err(|_| "Unable to protect the household key with the passcode.".to_string())?;
    Ok(PasscodeSlot {
        version: SLOT_VERSION, kdf: KDF_NAME.into(), kdf_version: KDF_VERSION,
        memory_ki_b: MEMORY_KI_B, iterations: ITERATIONS, parallelism: PARALLELISM,
        salt: STANDARD.encode(salt), nonce: STANDARD.encode(nonce), wrapped_key: STANDARD.encode(wrapped),
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn wrap(household_key: &[u8; 32], passcode: &str, budget_id: &str) -> Result<PasscodeSlot, String> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut salt).and_then(|()| getrandom::getrandom(&mut nonce))
        .map_err(|_| "Secure operating system randomness is unavailable; the passcode was not set.".to_string())?;
    wrap_with_entropy(household_key, passcode, budget_id, salt, nonce)
}

/// Returns the household key for a passcode, or `AUTH_ERROR` when either is wrong.
pub fn unwrap_key(slot: &PasscodeSlot, passcode: &str, budget_id: &str) -> Result<Zeroizing<[u8; 32]>, String> {
    validate_slot(slot)?;
    check_budget_id(budget_id)?;
    if passcode.is_empty() { return Err(AUTH_ERROR.into()); }
    if passcode.len() > PASSCODE_MAX_BYTES { return Err(AUTH_ERROR.into()); }
    let salt = STANDARD.decode(&slot.salt).map_err(|_| AUTH_ERROR)?;
    let nonce = STANDARD.decode(&slot.nonce).map_err(|_| AUTH_ERROR)?;
    let wrapped = STANDARD.decode(&slot.wrapped_key).map_err(|_| AUTH_ERROR)?;
    let ke = derive(passcode, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(ke.as_ref()).map_err(|_| AUTH_ERROR)?;
    // The decrypted bytes are the household key: wiped when dropped, on success as on failure.
    let plain = Zeroizing::new(cipher
        .decrypt(Nonce::from_slice(&nonce), Payload { msg: &wrapped, aad: &aad(budget_id)? })
        .map_err(|_| AUTH_ERROR)?);
    let mut key = Zeroizing::new([0u8; 32]);
    if plain.len() != 32 { return Err(AUTH_ERROR.into()); }
    key.copy_from_slice(&plain);
    Ok(key)
}

/// Structural validation, independent of the passcode: exactly the fixed version, KDF and cost,
/// so a tampered or future slot is refused before any Argon2 work is spent on it.
pub fn validate_slot(slot: &PasscodeSlot) -> Result<(), String> {
    if slot.version != SLOT_VERSION || slot.kdf != KDF_NAME || slot.kdf_version != KDF_VERSION
        || slot.memory_ki_b != MEMORY_KI_B || slot.iterations != ITERATIONS || slot.parallelism != PARALLELISM
        || slot.salt.len() != 24 || slot.nonce.len() != 16 || slot.wrapped_key.len() != 64
    {
        return Err("Invalid or unsupported household passcode record.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] { (0..32).collect::<Vec<u8>>().try_into().unwrap() }

    #[test]
    fn wrap_and_unwrap_round_trip_and_wrong_passcode() {
        let slot = wrap_with_entropy(&key(), "correct horse battery staple", "94dc206f-8f43-4e28-941b-a9bd4c4847da", [1; 16], [2; 12]).unwrap();
        let opened = unwrap_key(&slot, "correct horse battery staple", "94dc206f-8f43-4e28-941b-a9bd4c4847da").unwrap();
        assert_eq!(opened.as_ref(), &key());
        assert_eq!(unwrap_key(&slot, "correct horse battery", "94dc206f-8f43-4e28-941b-a9bd4c4847da").unwrap_err(), AUTH_ERROR);
        assert_eq!(unwrap_key(&slot, "", "94dc206f-8f43-4e28-941b-a9bd4c4847da").unwrap_err(), AUTH_ERROR);
    }

    #[test]
    fn slot_binds_the_household_identity() {
        assert!(wrap_with_entropy(&key(), "purple cloud lantern", "", [1; 16], [2; 12]).is_err());
        let slot = wrap_with_entropy(&key(), "purple cloud lantern", "0f9e5e52", [1; 16], [2; 12]).unwrap();
        // The same passcode under another household's identity must not open it.
        assert_eq!(unwrap_key(&slot, "purple cloud lantern", "0f9e5e53").unwrap_err(), AUTH_ERROR);
    }

    #[test]
    fn tampered_records_are_refused_without_derivation() {
        let slot = wrap_with_entropy(&key(), "copper kettle morning", "0f9e5e52", [1; 16], [2; 12]).unwrap();
        let mut cheap = slot.clone();
        cheap.memory_ki_b = 1024;
        assert_eq!(validate_slot(&cheap).unwrap_err(), "Invalid or unsupported household passcode record.");
        let mut wrong_len = slot.clone();
        wrong_len.wrapped_key.truncate(63);
        assert!(validate_slot(&wrong_len).is_err());
        let mut flipped = slot.clone();
        let mut bytes = STANDARD.decode(&flipped.wrapped_key).unwrap();
        bytes[0] ^= 1;
        flipped.wrapped_key = STANDARD.encode(bytes);
        assert_eq!(unwrap_key(&flipped, "copper kettle morning", "0f9e5e52").unwrap_err(), AUTH_ERROR);
    }

    #[test]
    fn passcode_policy_rejects_the_obvious() {
        for bad in ["short", "12345678", "password123", "BC1-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"] {
            assert!(wrap_with_entropy(&key(), bad, "0f9e5e52", [1; 16], [2; 12]).is_err(), "{bad}");
        }
        assert!(wrap_with_entropy(&key(), "quiet meadow ticket", "0f9e5e52", [1; 16], [2; 12]).is_ok());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn os_entropy_wrap_round_trips_with_fresh_salts() {
        let passcode = "amber harbour visitor";
        let a = wrap(&key(), passcode, "0f9e5e52").unwrap();
        let b = wrap(&key(), passcode, "0f9e5e52").unwrap();
        assert_ne!(a.salt, b.salt, "each slot must use a fresh salt");
        assert_eq!(unwrap_key(&a, passcode, "0f9e5e52").unwrap().as_ref(), &key());
        assert_eq!(unwrap_key(&b, passcode, "0f9e5e52").unwrap().as_ref(), &key());
    }

    #[test]
    fn slot_json_round_trips_with_exact_fields() {
        let slot = wrap_with_entropy(&key(), "silver meadow compass", "0f9e5e52", [1; 16], [2; 12]).unwrap();
        let json = serde_json::to_value(&slot).unwrap();
        assert_eq!(json["kdfVersion"], 19);
        assert_eq!(json["memoryKiB"], 65536);
        let back: PasscodeSlot = serde_json::from_value(json).unwrap();
        assert_eq!(back, slot);
        let extra = serde_json::from_value::<PasscodeSlot>(serde_json::json!({
            "version": 1, "kdf": "argon2id", "kdfVersion": 19, "memoryKiB": 65536, "iterations": 3, "parallelism": 4,
            "salt": slot.salt, "nonce": slot.nonce, "wrappedKey": slot.wrapped_key, "unexpected": true
        }));
        assert!(extra.is_err(), "unknown fields must be refused");
    }
}
