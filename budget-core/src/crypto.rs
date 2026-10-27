//! `.bgt` line encryption: Blowfish-ECB with zero padding and lowercase hex (`#db*encrypted`),
//! plus the read-only legacy XOR scheme (`#db*Encrypted`).

use blowfish::Blowfish;
use cipher::generic_array::GenericArray;
use cipher::{BlockDecrypt, BlockEncrypt, KeyInit};

pub const HEADER_BLOWFISH: &str = "#db*encrypted";
pub const HEADER_XOR: &str = "#db*Encrypted";
const MAX_PASSWD: usize = 56;

pub struct LineCipher {
    bf: Blowfish,
}

impl LineCipher {
    /// `new Blowfish().setPassword(password)`: UTF-8 bytes, truncated to 56. The original accepts an
    /// empty key (no key mixing); this port rejects it, as the UI does ("Empty password!").
    pub fn new(password: &str) -> Result<LineCipher, String> {
        let mut key = password.as_bytes().to_vec();
        key.truncate(MAX_PASSWD);
        if key.is_empty() { return Err("Empty password!".into()); }
        if key.len() < 4 { key.resize(4, 0); } // ponytail: crate needs >= 4 bytes; Java cycles short keys over 72 bytes, equivalent only for len >= 4
        let bf = Blowfish::new_from_slice(&key).map_err(|e| e.to_string())?;
        Ok(LineCipher { bf })
    }

    /// `bytes2hex(bf.encrypt(s.getBytes()))`.
    pub fn encrypt_line(&self, s: &str) -> String {
        let mut bytes = s.as_bytes().to_vec();
        let pad = (8 - bytes.len() % 8) % 8;
        bytes.extend(std::iter::repeat(0u8).take(pad));
        for chunk in bytes.chunks_mut(8) {
            self.bf.encrypt_block(GenericArray::from_mut_slice(chunk));
        }
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// `new String(bf.decrypt(hex2bytes(line)), 0, firstNul)`.
    pub fn decrypt_line(&self, hex: &str) -> Result<String, String> {
        let hex = if hex.len() % 2 == 1 { format!("0{hex}") } else { hex.to_string() };
        let mut bytes = Vec::with_capacity(hex.len() / 2);
        for i in (0..hex.len()).step_by(2) {
            let b = u8::from_str_radix(hex.get(i..i + 2).ok_or("bad hex")?, 16).map_err(|e| format!("NumberFormatException: {e}"))?;
            bytes.push(b);
        }
        if bytes.len() % 8 != 0 { return Err("ArrayIndexOutOfBoundsException: cipher text is not a multiple of 8 bytes".into()); }
        for chunk in bytes.chunks_mut(8) {
            self.bf.decrypt_block(GenericArray::from_mut_slice(chunk));
        }
        let n = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        Ok(String::from_utf8_lossy(&bytes[..n]).into_owned())
    }
}

/// Legacy symmetric XOR: `c = s[i] ^ password[i % len]`, except that positions producing `\n`/`\r`
/// keep the original character.
pub fn xor_line(s: &str, password: &str) -> String {
    if password.is_empty() { return s.to_string(); }
    let pw: Vec<char> = password.chars().collect();
    s.chars().enumerate().map(|(i, c)| {
        let x = (c as u32) ^ (pw[i % pw.len()] as u32);
        if x == 10 || x == 13 { c } else { char::from_u32(x).unwrap_or('?') }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blowfish_known_answer_and_roundtrip() {
        // Schneier's test vector: key 0000000000000000, plaintext 0000000000000000 -> 4EF997456198DD78
        let c = LineCipher::new("\0\0\0\0\0\0\0\0").unwrap();
        assert_eq!(c.encrypt_line("\0\0\0\0\0\0\0\0"), "4ef997456198dd78");
        assert_eq!(c.encrypt_line(""), ""); // like Java: nothing to pad, nothing to encode
        let c = LineCipher::new("secret").unwrap();
        let line = "-80.0, Car Insurance, 20040825, 1m, 1, 20040825, 16, paper.gif , $Auto";
        let enc = c.encrypt_line(line);
        assert_eq!(enc.len() % 16, 0);
        assert_eq!(c.decrypt_line(&enc).unwrap(), line);
        assert_eq!(c.decrypt_line("00").unwrap_err().contains("multiple of 8"), true);
        assert!(LineCipher::new("").is_err());
    }

    #[test]
    fn xor_is_an_involution() {
        let s = "v,2 hello";
        assert_eq!(xor_line(&xor_line(s, "pw"), "pw"), s);
    }
}
