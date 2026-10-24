use std::{
    io::{self, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use stem_core::crypto::{
    self,
    stream::{DecryptStream, EncryptStream, CHUNK_BYTES},
    Credential,
};
const PASSWORD: &str = "Deliberate offline stream test passphrase";
fn seal(bytes: &[u8]) -> (Vec<u8>, String) {
    let mut writer = EncryptStream::new(Vec::new(), PASSWORD).unwrap();
    writer.write_all(bytes).unwrap();
    let (bytes, key) = writer.finish().unwrap();
    (bytes, key.to_string())
}
fn open(bytes: &[u8], key: &str, budget: u64) -> io::Result<Vec<u8>> {
    let mut reader =
        DecryptStream::new(bytes, Credential::Recovery(key), budget).map_err(io::Error::other)?;
    let mut output = Vec::new();
    reader.read_to_end(&mut output)?;
    reader.finish().map_err(io::Error::other)?;
    Ok(output)
}
#[test]
fn streaming_password_and_recovery_roundtrip_across_arbitrary_writes_and_flushes() {
    let source = "Original ∫x² dx; deliberately incorrect 1+1=3\n"
        .repeat(4000)
        .into_bytes();
    let mut stream = EncryptStream::new(Vec::new(), PASSWORD).unwrap();
    for part in source.chunks(197) {
        stream.write_all(part).unwrap();
        if part[0] % 7 == 0 {
            stream.flush().unwrap();
        }
    }
    let (encrypted, recovery) = stream.finish().unwrap();
    assert_eq!(&encrypted[..8], b"STEMENC2");
    assert_eq!(
        open(&encrypted, &recovery, source.len() as u64).unwrap(),
        source
    );
    let mut reader = DecryptStream::new(
        encrypted.as_slice(),
        Credential::Password(PASSWORD),
        source.len() as u64,
    )
    .unwrap();
    let mut plain = Vec::new();
    reader.read_to_end(&mut plain).unwrap();
    assert_eq!(plain, source);
    assert!(DecryptStream::new(
        encrypted.as_slice(),
        Credential::Password("wrong credential"),
        source.len() as u64
    )
    .is_err());
    assert!(DecryptStream::new(encrypted.as_slice(), Credential::Recovery("z"), u64::MAX).is_err());
    assert!(crypto::decrypt(&encrypted, Credential::Recovery(&recovery)).is_err());
    // v1 cannot misread v2
}
#[test]
fn reordered_duplicated_truncated_spliced_or_appended_frames_never_finish_successfully() {
    let source = vec![b'x'; CHUNK_BYTES * 2 + 23];
    let (encrypted, key) = seal(&source);
    let header = 187;
    let frame = 5 + CHUNK_BYTES + 16;
    let mut variants = Vec::new();
    let mut swapped = encrypted.clone();
    swapped[header..header + frame].copy_from_slice(&encrypted[header + frame..header + frame * 2]);
    swapped[header + frame..header + frame * 2].copy_from_slice(&encrypted[header..header + frame]);
    variants.push(swapped);
    let mut duplicate = encrypted.clone();
    duplicate.splice(
        header + frame..header + frame,
        encrypted[header..header + frame].iter().copied(),
    );
    variants.push(duplicate);
    let mut altered = encrypted.clone();
    altered[header + 19] ^= 1;
    variants.push(altered);
    let mut flag = encrypted.clone();
    flag[header + 4] = 1;
    variants.push(flag);
    let mut length = encrypted.clone();
    length[header..header + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    variants.push(length);
    let mut trailing = encrypted.clone();
    trailing.push(0);
    variants.push(trailing);
    variants.push(encrypted[..encrypted.len() - 1].to_vec());
    variants.push(encrypted[..header + frame * 2].to_vec());
    let (other, _) = seal(&source);
    let mut splice = encrypted.clone();
    splice[header..header + frame].copy_from_slice(&other[header..header + frame]);
    variants.push(splice);
    for variant in variants {
        assert!(open(&variant, &key, source.len() as u64).is_err());
    }
    assert_eq!(open(&encrypted, &key, source.len() as u64).unwrap(), source);
}
#[test]
fn authenticated_prefix_requires_explicit_finality_and_plaintext_budget_cannot_be_exceeded() {
    let (encrypted, key) = seal(&vec![7; CHUNK_BYTES + 3]);
    let mut reader = DecryptStream::new(
        &encrypted[..187 + 5 + CHUNK_BYTES + 16],
        Credential::Recovery(&key),
        u64::MAX,
    )
    .unwrap();
    let mut prefix = vec![0; CHUNK_BYTES];
    reader.read_exact(&mut prefix).unwrap();
    assert_eq!(prefix, vec![7; CHUNK_BYTES]);
    assert!(reader.finish().is_err());
    assert!(open(&encrypted, &key, CHUNK_BYTES as u64 + 2).is_err());
    let mut broken =
        DecryptStream::new(encrypted.as_slice(), Credential::Recovery(&key), 0).unwrap();
    assert!(broken.read(&mut [0]).is_err());
    assert!(broken.read(&mut [0]).is_err());
    let (empty, key) = seal(&[]);
    assert!(open(&empty, &key, 0).unwrap().is_empty());
}
struct FailingSink {
    fail: Arc<AtomicBool>,
    writes: usize,
}
impl Write for FailingSink {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if self.fail.load(Ordering::Acquire) {
            return Err(io::Error::other("synthetic sink failure"));
        }
        self.writes += 1;
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.fail.load(Ordering::Acquire) {
            Err(io::Error::other("synthetic flush failure"))
        } else {
            Ok(())
        }
    }
}
#[test]
fn sink_failure_poisoning_prevents_retry_or_final_recovery_receipt() {
    let fail = Arc::new(AtomicBool::new(false));
    let mut writer = EncryptStream::new(
        FailingSink {
            fail: fail.clone(),
            writes: 0,
        },
        PASSWORD,
    )
    .unwrap();
    fail.store(true, Ordering::Release);
    assert!(writer.write_all(&vec![1; CHUNK_BYTES]).is_err());
    fail.store(false, Ordering::Release);
    assert!(writer.write_all(b"retry").is_err());
    assert!(writer.flush().is_err());
    assert!(writer.finish().is_err());
    let fail = Arc::new(AtomicBool::new(false));
    let mut writer = EncryptStream::new(
        FailingSink {
            fail: fail.clone(),
            writes: 0,
        },
        PASSWORD,
    )
    .unwrap();
    writer.write_all(b"buffered input").unwrap();
    fail.store(true, Ordering::Release);
    assert!(writer.flush().is_err());
    fail.store(false, Ordering::Release);
    assert!(writer.finish().is_err());
}
#[test]
fn streaming_above_old_64_mib_envelope_limit_uses_only_bounded_io_frames() {
    use sha2::{Digest, Sha256};
    struct BoundedWriter(std::fs::File);
    impl Write for BoundedWriter {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            assert!(b.len() <= CHUNK_BYTES + 16);
            self.0.write(b)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }
    struct HashSink(Sha256);
    impl Write for HashSink {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.update(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut writer = EncryptStream::new(BoundedWriter(file.reopen().unwrap()), PASSWORD).unwrap();
    let part = (0..CHUNK_BYTES)
        .map(|i| (i % 251) as u8)
        .collect::<Vec<_>>();
    let mut expected = Sha256::new();
    for _ in 0..1025 {
        writer.write_all(&part).unwrap();
        expected.update(&part);
    }
    let (_, key) = writer.finish().unwrap();
    assert!(file.as_file().metadata().unwrap().len() > 64 * 1024 * 1024);
    let mut reader = DecryptStream::new(
        file.reopen().unwrap(),
        Credential::Recovery(&key),
        (CHUNK_BYTES * 1025) as u64,
    )
    .unwrap();
    let mut hash = HashSink(Sha256::new());
    assert_eq!(
        io::copy(&mut reader, &mut hash).unwrap(),
        (CHUNK_BYTES * 1025) as u64
    );
    reader.finish().unwrap();
    assert_eq!(hash.0.finalize(), expected.finalize());
}

#[test]
fn envelope_and_frames_are_readable_by_rustcrypto_primitives_independent_of_the_stream_reader() {
    use aead::stream::DecryptorBE32;
    use chacha20poly1305::{
        aead::{Aead, KeyInit, Payload},
        XChaCha20Poly1305, XNonce,
    };
    let original = vec![13; CHUNK_BYTES + 31];
    let (bytes, recovery) = seal(&original);
    let key = (0..32)
        .map(|i| u8::from_str_radix(&recovery[i * 2..i * 2 + 2], 16).unwrap())
        .collect::<Vec<_>>();
    let wrapping = XChaCha20Poly1305::new_from_slice(&key).unwrap();
    let data_key = wrapping
        .decrypt(
            XNonce::from_slice(&bytes[96..120]),
            Payload {
                msg: &bytes[120..168],
                aad: &bytes[..24],
            },
        )
        .unwrap();
    let mut decryptor = Some(DecryptorBE32::from_aead(
        XChaCha20Poly1305::new_from_slice(&data_key).unwrap(),
        (&bytes[168..187]).into(),
    ));
    let mut offset = 187;
    let mut plain = Vec::new();
    loop {
        let frame = &bytes[offset..offset + 5];
        let len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
        offset += 5;
        let mut aad = bytes[..187].to_vec();
        aad.extend_from_slice(frame);
        let payload = Payload {
            msg: &bytes[offset..offset + len],
            aad: &aad,
        };
        offset += len;
        let part = if frame[4] == 1 {
            decryptor.take().unwrap().decrypt_last(payload).unwrap()
        } else {
            decryptor.as_mut().unwrap().decrypt_next(payload).unwrap()
        };
        plain.extend_from_slice(&part);
        if frame[4] == 1 {
            break;
        }
    }
    assert_eq!(offset, bytes.len());
    assert_eq!(plain, original);
}
