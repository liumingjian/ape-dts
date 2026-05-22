use byteorder::{BigEndian, ByteOrder};
use hmac::{Hmac, Mac, NewMac};
use md5::{Digest as Md5Digest, Md5};
use sha2::Sha256;

const SHA256_PREFIX: &[u8] = b"sha256";
const MD5_PREFIX: &[u8] = b"md5";
const CLIENT_KEY: &[u8] = b"Client Key";
const SERVER_KEY: &[u8] = b"Sever Key";
const DEFAULT_ITERATIONS: u32 = 2048;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordStorageMethod {
    Plain,
    Md5,
    Sha256,
    Sha256Rfc,
}

impl PasswordStorageMethod {
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Plain),
            1 => Some(Self::Md5),
            2 => Some(Self::Sha256),
            6 => Some(Self::Sha256Rfc),
            _ => None,
        }
    }
}

pub fn sha256_md5_password(user: &str, password: &[u8], salt: &[u8; 4]) -> Vec<u8> {
    let mut md5 = Md5::new();
    md5.update(password);
    md5.update(user.as_bytes());
    let first = hex_lower(md5.finalize_reset().as_ref());

    let mut sha = Sha256::new();
    sha.update(first.as_bytes());
    sha.update(salt);
    let mut out = Vec::with_capacity(SHA256_PREFIX.len() + 64);
    out.extend_from_slice(SHA256_PREFIX);
    out.extend_from_slice(hex_lower(sha.finalize().as_ref()).as_bytes());
    out
}

pub fn md5_sha256_password(password: &[u8], random64: &str, salt: &[u8; 4]) -> Vec<u8> {
    let derived = gaussdb_sha256_password(password, random64, None, DEFAULT_ITERATIONS);
    let source = format!(
        "{}{}{}",
        random64,
        hex_hmac(password, SERVER_KEY),
        sha256_hex(&derived)
    );

    let mut md5 = Md5::new();
    md5.update(source.as_bytes());
    md5.update(salt);

    let mut out = Vec::with_capacity(MD5_PREFIX.len() + 32);
    out.extend_from_slice(MD5_PREFIX);
    out.extend_from_slice(hex_lower(md5.finalize().as_ref()).as_bytes());
    out
}

pub fn gaussdb_sha256_password(
    password: &[u8],
    random64: &str,
    token8: Option<&str>,
    iterations: u32,
) -> Vec<u8> {
    let salt = hex_to_bytes(random64).unwrap_or_default();
    let key = pbkdf2_hmac_sha1(password, &salt, iterations.max(1), 32);
    let client_key = hmac_sha256(&key, CLIENT_KEY);
    let stored_key = Sha256::digest(&client_key);
    let token = hex_to_bytes(token8.unwrap_or_default()).unwrap_or_default();

    let client_signature = hmac_sha256(stored_key.as_ref(), &token);
    let proof = xor_bytes(&client_signature, &client_key);
    hex_lower(&proof).into_bytes()
}

fn pbkdf2_hmac_sha1(password: &[u8], salt: &[u8], iterations: u32, len: usize) -> Vec<u8> {
    let mut out = vec![0_u8; len];
    let mut block = 1_u32;
    let mut offset = 0_usize;

    while offset < len {
        let mut input = Vec::with_capacity(salt.len() + 4);
        input.extend_from_slice(salt);
        let mut block_buf = [0_u8; 4];
        BigEndian::write_u32(&mut block_buf, block);
        input.extend_from_slice(&block_buf);

        let mut u = hmac_sha1(password, &input);
        let mut t = u.clone();
        for _ in 1..iterations {
            u = hmac_sha1(password, &u);
            for (target, source) in t.iter_mut().zip(&u) {
                *target ^= source;
            }
        }

        let take = t.len().min(len - offset);
        out[offset..offset + take].copy_from_slice(&t[..take]);
        offset += take;
        block += 1;
    }

    out
}

fn hmac_sha1(key: &[u8], data: &[u8]) -> Vec<u8> {
    type HmacSha1 = Hmac<sha1::Sha1>;
    let mut mac = HmacSha1::new_varkey(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_varkey(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex_hmac(key: &[u8], data: &[u8]) -> String {
    hex_lower(&hmac_sha256(key, data))
}

fn sha256_hex(data: &[u8]) -> String {
    hex_lower(Sha256::digest(data).as_ref())
}

fn xor_bytes(left: &[u8], right: &[u8]) -> Vec<u8> {
    left.iter().zip(right).map(|(l, r)| l ^ r).collect()
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{:02x}", byte);
    }
    out
}

fn hex_to_bytes(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for idx in (0..value.len()).step_by(2) {
        let byte = u8::from_str_radix(&value[idx..idx + 2], 16).ok()?;
        out.push(byte);
    }
    Some(out)
}
