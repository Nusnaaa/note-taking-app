//! STEMENC2: RustCrypto STREAM BE32/XChaCha20Poly1305 with independent password
//! and offline recovery-key wraps. Bounded frames; no library-sized plaintext.
//! Authenticated prefixes are NOT proof of whole-stream completion. Consumers
//! must read to EOF/finish before committing or publishing restored state.
use super::{cipher, derive, random, validate_password, Credential};
use crate::{Error, Result};
use aead::stream::{DecryptorBE32, EncryptorBE32};
use chacha20poly1305::{
    aead::{Aead, Payload},
    XChaCha20Poly1305, XNonce,
};
use std::io::{self, Read, Write};
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &[u8; 8] = b"STEMENC2";
const HEADER: usize = 187; // magic/salt + two independent wraps + STREAM nonce
pub const CHUNK_BYTES: usize = 64 * 1024;
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "encrypted stream integrity")
}
fn aad(header: &[u8; HEADER], frame: &[u8; 5]) -> [u8; HEADER + 5] {
    let mut aad = [0; HEADER + 5];
    aad[..HEADER].copy_from_slice(header);
    aad[HEADER..].copy_from_slice(frame);
    aad
}
fn recovery_text(bytes: &[u8]) -> Zeroizing<String> {
    Zeroizing::new(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
/// Writing/flush never acknowledges a complete archive. Only finish returns the
/// offline recovery key. An I/O/crypto failure poisons this object permanently;
/// callers discard partial encrypted output instead of retrying a nonce/state.
pub struct EncryptStream<W: Write> {
    sink: W,
    encryptor: Option<EncryptorBE32<XChaCha20Poly1305>>,
    header: [u8; HEADER],
    buffer: Zeroizing<Vec<u8>>,
    recovery: Zeroizing<String>,
    failed: bool,
}
impl<W: Write> EncryptStream<W> {
    pub fn new(mut sink: W, password: &str) -> Result<Self> {
        validate_password(password)?;
        let salt = random::<16>()?;
        let password_key = derive(password, &salt)?;
        let data_key = Zeroizing::new(random::<32>()?);
        let recovery = Zeroizing::new(random::<32>()?);
        let p_nonce = random::<24>()?;
        let r_nonce = random::<24>()?;
        let stream_nonce = random::<19>()?;
        let mut header = [0; HEADER];
        header[..8].copy_from_slice(MAGIC);
        header[8..24].copy_from_slice(&salt);
        let p_wrap = cipher(&password_key[..])?
            .encrypt(
                XNonce::from_slice(&p_nonce),
                Payload {
                    msg: &data_key[..],
                    aad: &header[..24],
                },
            )
            .map_err(|_| Error::Integrity)?;
        let r_wrap = cipher(&recovery[..])?
            .encrypt(
                XNonce::from_slice(&r_nonce),
                Payload {
                    msg: &data_key[..],
                    aad: &header[..24],
                },
            )
            .map_err(|_| Error::Integrity)?;
        header[24..48].copy_from_slice(&p_nonce);
        header[48..96].copy_from_slice(&p_wrap);
        header[96..120].copy_from_slice(&r_nonce);
        header[120..168].copy_from_slice(&r_wrap);
        header[168..].copy_from_slice(&stream_nonce);
        let encryptor = EncryptorBE32::from_aead(cipher(&data_key[..])?, (&stream_nonce).into());
        sink.write_all(&header)?;
        Ok(Self {
            sink,
            encryptor: Some(encryptor),
            header,
            buffer: Zeroizing::new(Vec::with_capacity(CHUNK_BYTES)),
            recovery: recovery_text(&recovery[..]),
            failed: false,
        })
    }
    fn emit(&mut self, last: bool) -> io::Result<()> {
        if self.failed {
            return Err(invalid());
        }
        // Mark failed BEFORE touching stream state or caller-supplied I/O.
        self.failed = true;
        let mut frame = [0; 5];
        frame[..4].copy_from_slice(&((self.buffer.len() + 16) as u32).to_be_bytes());
        frame[4] = u8::from(last);
        let aad = aad(&self.header, &frame);
        let payload = Payload {
            msg: &self.buffer,
            aad: &aad,
        };
        let encrypted = if last {
            self.encryptor
                .take()
                .ok_or_else(invalid)?
                .encrypt_last(payload)
        } else {
            self.encryptor
                .as_mut()
                .ok_or_else(invalid)?
                .encrypt_next(payload)
        }
        .map_err(|_| invalid())?;
        self.sink.write_all(&frame)?;
        self.sink.write_all(&encrypted)?;
        self.buffer.zeroize();
        self.buffer.clear();
        self.failed = false;
        Ok(())
    }
    pub fn finish(mut self) -> Result<(W, Zeroizing<String>)> {
        self.emit(true)?;
        self.failed = true;
        self.sink.flush()?;
        Ok((self.sink, self.recovery))
    }
}
impl<W: Write> Write for EncryptStream<W> {
    fn write(&mut self, mut bytes: &[u8]) -> io::Result<usize> {
        if self.failed {
            return Err(invalid());
        }
        let count = bytes.len();
        while !bytes.is_empty() {
            let next = bytes.len().min(CHUNK_BYTES - self.buffer.len());
            self.buffer.extend_from_slice(&bytes[..next]);
            bytes = &bytes[next..];
            if self.buffer.len() == CHUNK_BYTES {
                self.emit(false)?;
            }
        }
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.failed {
            return Err(invalid());
        }
        if !self.buffer.is_empty() {
            self.emit(false)?;
        }
        self.failed = true;
        self.sink.flush()?;
        self.failed = false;
        Ok(())
    }
}
/// Reads only individually authenticated frames. A caller-supplied plaintext
/// budget is enforced before releasing each frame. EOF requires authenticated
/// finality AND exact underlying EOF: truncation/appending are errors.
pub struct DecryptStream<R: Read> {
    source: R,
    decryptor: Option<DecryptorBE32<XChaCha20Poly1305>>,
    header: [u8; HEADER],
    buffer: Zeroizing<Vec<u8>>,
    offset: usize,
    remaining: u64,
    finished: bool,
    failed: bool,
}
impl<R: Read> DecryptStream<R> {
    pub fn new(
        mut source: R,
        credential: Credential<'_>,
        max_plaintext_bytes: u64,
    ) -> Result<Self> {
        let mut header = [0; HEADER];
        source
            .read_exact(&mut header)
            .map_err(|_| Error::Integrity)?;
        if &header[..8] != MAGIC {
            return Err(Error::Unsupported);
        }
        let wrapping_key = match credential {
            Credential::Password(p) => {
                if p.len() > 1024 {
                    return Err(Error::Integrity);
                }
                derive(p, &header[8..24])?
            }
            Credential::Recovery(h) => {
                if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(Error::Integrity);
                }
                let mut key = Zeroizing::new([0; 32]);
                for (i, k) in key.iter_mut().enumerate() {
                    *k = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16)
                        .map_err(|_| Error::Integrity)?;
                }
                key
            }
        };
        let (nonce, wrapped) = match credential {
            Credential::Password(_) => (&header[24..48], &header[48..96]),
            Credential::Recovery(_) => (&header[96..120], &header[120..168]),
        };
        let data_key = Zeroizing::new(
            cipher(&wrapping_key[..])?
                .decrypt(
                    XNonce::from_slice(nonce),
                    Payload {
                        msg: wrapped,
                        aad: &header[..24],
                    },
                )
                .map_err(|_| Error::Integrity)?,
        );
        let decryptor = DecryptorBE32::from_aead(cipher(&data_key)?, (&header[168..]).into());
        Ok(Self {
            source,
            decryptor: Some(decryptor),
            header,
            buffer: Zeroizing::new(Vec::new()),
            offset: 0,
            remaining: max_plaintext_bytes,
            finished: false,
            failed: false,
        })
    }
    fn frame(&mut self) -> io::Result<()> {
        self.failed = true;
        let mut frame = [0; 5];
        self.source.read_exact(&mut frame).map_err(|_| invalid())?;
        let len = u32::from_be_bytes(frame[..4].try_into().map_err(|_| invalid())?) as usize;
        if !(16..=CHUNK_BYTES + 16).contains(&len) || frame[4] > 1 || (len == 16 && frame[4] == 0) {
            return Err(invalid());
        }
        let plain_len = (len - 16) as u64;
        if plain_len > self.remaining {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "encrypted stream budget",
            ));
        }
        let mut encrypted = vec![0; len];
        self.source
            .read_exact(&mut encrypted)
            .map_err(|_| invalid())?;
        let aad = aad(&self.header, &frame);
        let payload = Payload {
            msg: &encrypted,
            aad: &aad,
        };
        let plain = if frame[4] == 1 {
            self.decryptor
                .take()
                .ok_or_else(invalid)?
                .decrypt_last(payload)
        } else {
            self.decryptor
                .as_mut()
                .ok_or_else(invalid)?
                .decrypt_next(payload)
        }
        .map_err(|_| invalid())?;
        self.buffer = Zeroizing::new(plain);
        self.offset = 0;
        self.remaining -= plain_len;
        if frame[4] == 1 {
            let mut tail = [0];
            if self.source.read(&mut tail)? != 0 {
                return Err(invalid());
            }
            self.finished = true;
        }
        self.failed = false;
        Ok(())
    }
    pub fn finish(mut self) -> Result<R> {
        io::copy(&mut self, &mut io::sink())?;
        Ok(self.source)
    }
}
impl<R: Read> Read for DecryptStream<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.failed {
            return Err(invalid());
        }
        if self.offset == self.buffer.len() {
            self.buffer.zeroize();
            self.buffer.clear();
            self.offset = 0;
            if self.finished {
                return Ok(0);
            }
            self.frame()?;
        }
        let count = out.len().min(self.buffer.len() - self.offset);
        out[..count].copy_from_slice(&self.buffer[self.offset..self.offset + count]);
        self.offset += count;
        Ok(count)
    }
}
