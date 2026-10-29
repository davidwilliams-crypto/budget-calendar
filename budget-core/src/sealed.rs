//! Version 1 standalone files: Argon2id v1.3 and AES-256-GCM over the whole plain file.
//! The ASCII header and canonical JSON metadata tuple are authenticated as associated data.
//! Legacy line ciphers remain read compatibility only at the `.bgt` boundary.
use aes_gcm::{aead::{Aead, Payload}, Aes256Gcm, KeyInit, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const HEADER: &str = "#db*sealed";
pub const MAX_PLAINTEXT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_PASSWORD_BYTES: usize = 64 * 1024;
pub const MAX_ENVELOPE_BYTES: usize = (MAX_PLAINTEXT_BYTES + 16).div_ceil(3) * 4 + 1024;
const AUTH_ERROR: &str = "Unable to unlock budget: incorrect password or damaged encrypted file.";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    version: u32,
    cipher: String,
    kdf: String,
    kdf_version: u32,
    memory_ki_b: u32,
    iterations: u32,
    parallelism: u32,
    salt: String,
    nonce: String,
    ciphertext: String,
}

/// Only the raw WASM ABI accepts entropy from its host, which has no OS imports.
/// The official browser wrapper must replace caller entropy using its CSPRNG on every export.
#[cfg(target_arch = "wasm32")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HostEntropy {
    pub salt: [u8; 16],
    pub nonce: [u8; 12],
}

fn check_password(password: &str) -> Result<(), String> {
    if password.is_empty() { return Err("Empty password!".into()); }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err("Password exceeds the supported 64 KiB UTF-8 limit.".into());
    }
    Ok(())
}

fn key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, String> {
    check_password(password)?;
    // RFC 9106's second recommended profile. Inputs cannot choose cheaper or costlier work.
    let params = Params::new(65536, 3, 4, Some(32)).map_err(|_| AUTH_ERROR)?;
    let mut memory = Vec::new();
    memory.try_reserve_exact(params.block_count()).map_err(|_| "Not enough memory to unlock the encrypted budget.")?;
    memory.resize(params.block_count(), argon2::Block::default());
    let mut memory = Zeroizing::new(memory);
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into_with_memory(password.as_bytes(), salt, key.as_mut(), memory.as_mut_slice())
        .map_err(|_| "Unable to derive the encrypted budget key.".to_string())?;
    Ok(key)
}

fn aad(e: &Envelope) -> Result<Vec<u8>, String> {
    let mut out = format!("{HEADER}\n").into_bytes();
    // A fixed-order tuple makes AAD independent of JSON property order and whitespace.
    out.extend(serde_json::to_vec(&(
        e.version, &e.cipher, &e.kdf, e.kdf_version, e.memory_ki_b,
        e.iterations, e.parallelism, &e.salt, &e.nonce,
    )).map_err(|_| AUTH_ERROR)?);
    Ok(out)
}

pub(crate) fn seal(plaintext: &str, password: &str) -> Result<String, String> {
    check_password(password)?;
    if plaintext.len() > MAX_PLAINTEXT_BYTES { return Err("Budget exceeds the encrypted file limit of 64 MiB.".into()); }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut salt = [0u8; 16];
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut salt).and_then(|()| getrandom::getrandom(&mut nonce))
            .map_err(|_| "Secure operating system randomness is unavailable; budget was not encrypted.".to_string())?;
        seal_with_entropy(plaintext, password, salt, nonce)
    }
    #[cfg(target_arch = "wasm32")]
    { Err("Password export requires fresh CSPRNG entropy from the browser host.".into()) }
}

pub(crate) fn seal_with_entropy(plaintext: &str, password: &str, salt: [u8; 16], nonce: [u8; 12]) -> Result<String, String> {
    check_password(password)?;
    if plaintext.len() > MAX_PLAINTEXT_BYTES { return Err("Budget exceeds the encrypted file limit of 64 MiB.".into()); }
    let mut e = Envelope {
        version: 1, cipher: "AES-256-GCM".into(), kdf: "argon2id".into(),
        kdf_version: 19, memory_ki_b: 65536, iterations: 3, parallelism: 4,
        salt: STANDARD.encode(salt), nonce: STANDARD.encode(nonce), ciphertext: String::new(),
    };
    let key = key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| AUTH_ERROR)?;
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext.as_bytes(), aad: &aad(&e)? })
        .map_err(|_| "Unable to encrypt budget.".to_string())?;
    e.ciphertext = STANDARD.encode(ciphertext);
    Ok(format!("{HEADER}\n{}", serde_json::to_string(&e).map_err(|_| AUTH_ERROR)?))
}

/// Authenticate every byte before returning plaintext. Validation and size limits precede the KDF.
pub(crate) fn open(text: &str, password: &str) -> Result<Zeroizing<String>, String> {
    if text.len() > MAX_ENVELOPE_BYTES { return Err("Encrypted budget exceeds the supported file limit.".into()); }
    check_password(password)?;
    let split = text.find(['\r', '\n']).ok_or(AUTH_ERROR)?;
    if &text[..split] != HEADER { return Err(AUTH_ERROR.into()); }
    let body = text[split..].trim_start_matches(['\r', '\n']);
    let e: Envelope = serde_json::from_str(body).map_err(|_| "Invalid or unsupported encrypted budget envelope.".to_string())?;
    if e.version != 1 || e.cipher != "AES-256-GCM" || e.kdf != "argon2id" || e.kdf_version != 19
        || e.memory_ki_b != 65536 || e.iterations != 3 || e.parallelism != 4
        || e.salt.len() != 24 || e.nonce.len() != 16 || e.ciphertext.len() > (MAX_PLAINTEXT_BYTES + 16).div_ceil(3) * 4
    { return Err("Invalid or unsupported encrypted budget parameters.".into()); }
    let salt = STANDARD.decode(&e.salt).map_err(|_| AUTH_ERROR)?;
    let nonce = STANDARD.decode(&e.nonce).map_err(|_| AUTH_ERROR)?;
    let ciphertext = STANDARD.decode(&e.ciphertext).map_err(|_| AUTH_ERROR)?;
    if salt.len() != 16 || nonce.len() != 12 || ciphertext.len() < 16 || ciphertext.len() > MAX_PLAINTEXT_BYTES + 16 {
        return Err(AUTH_ERROR.into());
    }
    let key = key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| AUTH_ERROR)?;
    let plain = Zeroizing::new(cipher.decrypt(Nonce::from_slice(&nonce), Payload { msg: &ciphertext, aad: &aad(&e)? })
        .map_err(|_| AUTH_ERROR)?);
    // Convert by copying so the temporary plaintext bytes are erased on every path.
    let text = std::str::from_utf8(&plain).map_err(|_| AUTH_ERROR)?.to_string();
    Ok(Zeroizing::new(text))
}
