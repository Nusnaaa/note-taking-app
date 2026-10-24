//! Trusted, worker-only backup transport. No review or sink capability comes
//! from ordinary notebook JSON. Registry locks never span Java callbacks.
use super::{hosts, JavaAuth};
use jni::{
    objects::{GlobalRef, JCharArray, JClass, JObject, JValue},
    sys::{jboolean, jcharArray, jlong, jobject},
    JNIEnv, JavaVM,
};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use stem_core::{backup::BackupOptions, host::BackupReview, processing::JobControl};
use zeroize::Zeroizing;

struct Entry {
    host: i64,
    review: Option<BackupReview>,
    control: Option<JobControl>,
    running: bool,
}
static JOBS: OnceLock<Mutex<BTreeMap<i64, Entry>>> = OnceLock::new();
static NEXT: AtomicI64 = AtomicI64::new(1);
fn jobs() -> &'static Mutex<BTreeMap<i64, Entry>> {
    JOBS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn fail(env: &mut JNIEnv<'_>) {
    if env.exception_check().unwrap_or(true) {
        let _ = env.exception_clear();
    }
    let _ = env.throw_new(
        "java/lang/IllegalStateException",
        "Native backup operation failed",
    );
}
pub(super) fn close(host: i64) {
    if let Ok(mut entries) = jobs().lock() {
        entries.retain(|_, entry| {
            if entry.host != host {
                return true;
            }
            if let Some(control) = &entry.control {
                control.cancel();
            }
            entry.running
        });
    }
}
struct Running(i64);
impl Drop for Running {
    fn drop(&mut self) {
        if let Ok(mut entries) = jobs().lock() {
            entries.remove(&self.0);
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_reviewBackup(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    ink: jboolean,
    assets: jboolean,
) -> jobject {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jobject> {
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let review = host
            .review_backup(BackupOptions {
                include_original_ink: ink != 0,
                include_authorised_source_assets: assets != 0,
            })
            .ok()?;
        let summary = review.summary();
        let omissions = env
            .new_object_array(
                i32::try_from(summary.omissions.len()).ok()?,
                "java/lang/String",
                JObject::null(),
            )
            .ok()?;
        for (i, text) in summary.omissions.iter().enumerate() {
            let text = env.new_string(text).ok()?;
            env.set_object_array_element(&omissions, i as i32, &text)
                .ok()?;
            env.delete_local_ref(text).ok()?;
        }
        let token = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .ok()?;
        let object = env
            .new_object(
                "org/stemnotebook/backend/NativeBridge$BackupReview",
                "(JJZJ[Ljava/lang/String;)V",
                &[
                    JValue::Long(handle),
                    JValue::Long(token),
                    JValue::Bool(summary.includes_original_ink.into()),
                    JValue::Long(i64::try_from(summary.source_assets).ok()?),
                    JValue::Object(&omissions),
                ],
            )
            .ok()?;
        // Pair insertion with host registry membership. Close removes membership
        // before clearing jobs, so a delayed review cannot resurrect a token.
        let registered = hosts().lock().ok()?;
        if !registered
            .get(&handle)
            .is_some_and(|h| Arc::ptr_eq(h, &host))
        {
            return None;
        }
        let mut entries = jobs().lock().ok()?;
        // Recheck after acquiring the registry: a delayed obsolete insertion
        // must never evict a newer review admitted after capture cancellation.
        if !review.is_current() {
            return None;
        }
        entries.retain(|_, e| e.host != handle || e.running);
        entries.insert(
            token,
            Entry {
                host: handle,
                review: Some(review),
                control: None,
                running: false,
            },
        );
        Some(object.into_raw())
    }));
    match result {
        Ok(Some(value)) => value,
        _ => {
            fail(&mut env);
            std::ptr::null_mut()
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_armBackup(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    budget: jlong,
) {
    let result = std::panic::catch_unwind(|| -> Option<()> {
        if !(1..=3_600_000).contains(&budget) {
            return None;
        }
        let mut entries = jobs().lock().ok()?;
        let entry = entries.get_mut(&token)?;
        if entry.host != handle
            || entry.running
            || entry.control.is_some()
            || !entry.review.as_ref()?.is_current()
        {
            return None;
        }
        entry.control = Some(JobControl::new(Duration::from_millis(budget as u64)).ok()?);
        Some(())
    });
    if !matches!(result, Ok(Some(()))) {
        fail(&mut env);
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_releaseBackup(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
) {
    let result = std::panic::catch_unwind(|| -> Option<()> {
        let mut entries = jobs().lock().ok()?;
        let Some(entry) = entries.get(&token) else {
            return Some(());
        };
        if entry.host != handle {
            return None;
        }
        if let Some(control) = &entry.control {
            control.cancel();
        }
        if !entry.running {
            entries.remove(&token);
        }
        Some(())
    });
    if !matches!(result, Ok(Some(()))) {
        fail(&mut env);
    }
}
struct JavaSink {
    vm: JavaVM,
    output: GlobalRef,
    retired: Arc<AtomicBool>,
}
impl JavaSink {
    fn callback(&self, bytes: Option<&[u8]>, method: &str) -> io::Result<()> {
        let result = (|| {
            let mut env = self.vm.attach_current_thread().ok()?;
            let result = env.with_local_frame(8, |env| -> jni::errors::Result<()> {
                if let Some(bytes) = bytes {
                    let array = env.byte_array_from_slice(bytes)?;
                    env.call_method(
                        self.output.as_obj(),
                        method,
                        "([B)V",
                        &[JValue::Object(&array)],
                    )?;
                } else {
                    env.call_method(self.output.as_obj(), method, "()V", &[])?;
                }
                Ok(())
            });
            if env.exception_check().unwrap_or(true) {
                let _ = env.exception_clear();
                return None;
            }
            result.ok()
        })();
        result.ok_or_else(|| io::Error::other("Backup output failed"))
    }
}
impl Write for JavaSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // Maximum authenticated frame: 64 KiB payload plus 16-byte tag.
        if bytes.len() > 65552 {
            return Err(io::Error::other("Backup frame bound"));
        }
        self.callback(Some(bytes), "write")?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.callback(None, "flush")
    }
}
impl Drop for JavaSink {
    fn drop(&mut self) {
        self.retired
            .store(self.callback(None, "retire").is_ok(), Ordering::Release);
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_runBackup(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    password: JCharArray<'_>,
    proof: JObject<'_>,
    confirmed: jboolean,
    output: JObject<'_>,
) -> jcharArray {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jcharArray> {
            let len = env.get_array_length(&password).ok()?;
            if len > 1024 || proof.is_null() || output.is_null() {
                return None;
            }
            let mut chars = Zeroizing::new(vec![0u16; len as usize]);
            env.get_char_array_region(&password, 0, &mut chars).ok()?;
            // Decode into already-zeroizing storage, including malformed-surrogate
            // rejection; the standard fallible String decoder can discard a partial
            // non-zeroizing allocation on error.
            let mut password = Zeroizing::new(String::with_capacity(chars.len() * 3));
            for scalar in char::decode_utf16(chars.iter().copied()) {
                password.push(scalar.ok()?);
            }
            let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
            let auth = JavaAuth {
                vm: env.get_java_vm().ok()?,
                proof: env.new_global_ref(proof).ok()?,
            };
            let retired = Arc::new(AtomicBool::new(false));
            let sink = JavaSink {
                vm: env.get_java_vm().ok()?,
                output: env.new_global_ref(output).ok()?,
                retired: Arc::clone(&retired),
            };
            let (review, control) = {
                let mut entries = jobs().lock().ok()?;
                let entry = entries.get_mut(&token)?;
                if entry.host != handle || entry.running {
                    return None;
                }
                let control = entry.control.clone()?;
                let review = entry.review.take()?;
                entry.running = true;
                (review, control)
            };
            let _running = Running(token);
            let receipt = host
                .finish_backup(
                    review,
                    &password,
                    Box::new(auth),
                    confirmed != 0,
                    sink,
                    &control,
                )
                .ok()?;
            if !retired.load(Ordering::Acquire) {
                return None;
            }
            let key = Zeroizing::new(receipt.recovery_key().encode_utf16().collect::<Vec<_>>());
            let array = env.new_char_array(i32::try_from(key.len()).ok()?).ok()?;
            env.set_char_array_region(&array, 0, &key).ok()?;
            Some(array.into_raw())
        }));
    match result {
        Ok(Some(value)) => value,
        _ => {
            fail(&mut env);
            std::ptr::null_mut()
        }
    }
}
