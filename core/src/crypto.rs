//! Versioned portable envelope using RustCrypto, not custom cryptographic primitives.
//! Passwords and offline recovery keys wrap a random data key independently.
use crate::{Error, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};
use rand::{rngs::OsRng, RngCore};
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"STEMENC1";
const MAX: usize = 64 * 1024 * 1024;
const HEADER: usize = 8 + 16 + 24 + 48 + 24 + 48 + 24;

pub struct Encrypted {
    pub bytes: Vec<u8>,
    pub recovery_key: Zeroizing<String>,
}

fn derive(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(65_536, 3, 1, Some(32)).map_err(|_| Error::Integrity)?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut *key)
        .map_err(|_| Error::Integrity)?;
    Ok(key)
}
fn cipher(key: &[u8]) -> Result<XChaCha20Poly1305> {
    XChaCha20Poly1305::new_from_slice(key).map_err(|_| Error::Integrity)
}
fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0; N];
    OsRng
        .try_fill_bytes(&mut b)
        .map_err(|_| Error::ResourceLimit)?;
    Ok(b)
}

pub fn encrypt(plain: &[u8], password: &str) -> Result<Encrypted> {
    if plain.len() > MAX {
        return Err(Error::ResourceLimit);
    }
    // A length/variety floor is not a guarantee of entropy. Recommend a generated
    // passphrase; a short numeric PIN is never accepted for offline encryption.
    if password.chars().count() < 12
        || password.len() > 1024
        || password.chars().all(|c| c.is_ascii_digit())
        || password
            .chars()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            < 6
    {
        return Err(Error::Invalid(
            "portable password; use a long distinct passphrase",
        ));
    }
    let salt = random::<16>()?;
    let password_key = derive(password, &salt)?;
    let data_key = Zeroizing::new(random::<32>()?);
    let recovery = Zeroizing::new(random::<32>()?);
    let p_nonce = random::<24>()?;
    let r_nonce = random::<24>()?;
    let nonce = random::<24>()?;
    let mut header = Vec::from(&MAGIC[..]);
    header.extend_from_slice(&salt);
    let p_wrap = cipher(&password_key[..])?
        .encrypt(
            XNonce::from_slice(&p_nonce),
            Payload {
                msg: &data_key[..],
                aad: &header,
            },
        )
        .map_err(|_| Error::Integrity)?;
    let r_wrap = cipher(&recovery[..])?
        .encrypt(
            XNonce::from_slice(&r_nonce),
            Payload {
                msg: &data_key[..],
                aad: &header,
            },
        )
        .map_err(|_| Error::Integrity)?;
    header.extend_from_slice(&p_nonce);
    header.extend_from_slice(&p_wrap);
    header.extend_from_slice(&r_nonce);
    header.extend_from_slice(&r_wrap);
    header.extend_from_slice(&nonce);
    let encrypted = cipher(&data_key[..])?
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: &header,
            },
        )
        .map_err(|_| Error::Integrity)?;
    header.extend_from_slice(&encrypted);
    let recovery_key = Zeroizing::new(
        recovery
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    );
    Ok(Encrypted {
        bytes: header,
        recovery_key,
    })
}
pub enum Credential<'a> {
    Password(&'a str),
    Recovery(&'a str),
}
pub fn decrypt(envelope: &[u8], credential: Credential<'_>) -> Result<Zeroizing<Vec<u8>>> {
    if envelope.len() < HEADER + 16 || envelope.len() > MAX + HEADER + 16 || &envelope[..8] != MAGIC
    {
        return Err(Error::Integrity);
    }
    let wrapping_key = match credential {
        Credential::Password(p) => {
            if p.len() > 1024 {
                return Err(Error::Integrity);
            }
            derive(p, &envelope[8..24])?
        }
        Credential::Recovery(h) => {
            if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::Integrity);
            }
            let mut key = Zeroizing::new([0u8; 32]);
            for (i, k) in key.iter_mut().enumerate() {
                *k = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).map_err(|_| Error::Integrity)?;
            }
            key
        }
    };
    let (nonce, wrapped) = match credential {
        Credential::Password(_) => (&envelope[24..48], &envelope[48..96]),
        Credential::Recovery(_) => (&envelope[96..120], &envelope[120..168]),
    };
    let data_key = Zeroizing::new(
        cipher(&wrapping_key[..])?
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: wrapped,
                    aad: &envelope[..24],
                },
            )
            .map_err(|_| Error::Integrity)?,
    );
    let plain = cipher(&data_key)?
        .decrypt(
            XNonce::from_slice(&envelope[168..192]),
            Payload {
                msg: &envelope[HEADER..],
                aad: &envelope[..HEADER],
            },
        )
        .map_err(|_| Error::Integrity)?;
    Ok(Zeroizing::new(plain))
}
