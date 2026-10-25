//! Trusted recovered-copy transport. Registry locks never span callbacks or stage I/O.
use super::{bounded_string, hosts, JavaAuth};
use jni::{
    objects::{GlobalRef, JCharArray, JClass, JObject, JString, JValue},
    sys::{jboolean, jlong, jstring},
    JNIEnv, JavaVM,
};
use std::{
    collections::BTreeMap,
    io::{self, Read},
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use stem_core::{crypto::Credential, host::RestoreReview, processing::JobControl};
use zeroize::Zeroizing;
struct Entry {
    host: i64,
    control: JobControl,
    review: Option<RestoreReview>,
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
        "Native restore operation failed",
    );
}
pub(super) fn close(host: i64) {
    let retired = if let Ok(mut entries) = jobs().lock() {
        let ids: Vec<_> = entries
            .iter()
            .filter_map(|(id, e)| {
                if e.host != host {
                    return None;
                }
                e.control.cancel();
                (!e.running).then_some(*id)
            })
            .collect();
        ids.into_iter()
            .filter_map(|id| entries.remove(&id))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    drop(retired);
}
struct Running {
    token: i64,
    keep: bool,
}
impl Drop for Running {
    fn drop(&mut self) {
        let retired = if let Ok(mut entries) = jobs().lock() {
            if self.keep
                && entries
                    .get(&self.token)
                    .is_some_and(|e| e.control.check().is_ok())
            {
                if let Some(entry) = entries.get_mut(&self.token) {
                    entry.running = false;
                }
                None
            } else {
                entries.remove(&self.token)
            }
        } else {
            None
        };
        drop(retired);
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_armRestore(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    budget: jlong,
) -> jlong {
    let result = std::panic::catch_unwind(|| -> Option<i64> {
        if !(1..=3_600_000).contains(&budget) {
            return None;
        }
        let hosts = hosts().lock().ok()?;
        let host = hosts.get(&handle)?;
        if host.restore_status().closed {
            return None;
        }
        let mut entries = jobs().lock().ok()?;
        if entries.values().any(|e| e.host == handle) {
            return None;
        }
        let token = super::allocate_handle(&NEXT)?;
        entries.insert(
            token,
            Entry {
                host: handle,
                control: JobControl::new(Duration::from_millis(budget as u64)).ok()?,
                review: None,
                running: false,
            },
        );
        Some(token)
    });
    match result {
        Ok(Some(id)) => id,
        _ => {
            fail(&mut env);
            0
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_releaseRestore(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
) {
    let result = std::panic::catch_unwind(|| -> Option<()> {
        let retired = {
            let mut entries = jobs().lock().ok()?;
            let Some(entry) = entries.get(&token) else {
                return Some(());
            };
            if entry.host != handle {
                return None;
            }
            entry.control.cancel();
            if entry.running {
                None
            } else {
                entries.remove(&token)
            }
        };
        drop(retired);
        Some(())
    });
    if !matches!(result, Ok(Some(()))) {
        fail(&mut env);
    }
}
struct JavaReader {
    vm: JavaVM,
    input: GlobalRef,
    retired: Arc<AtomicBool>,
}
impl Read for JavaReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        let result = (|| -> Option<usize> {
            let mut env = self.vm.attach_current_thread().ok()?;
            let result = env.with_local_frame(8, |env| -> jni::errors::Result<Option<usize>> {
                let length = output.len().min(65552);
                let array = env.new_byte_array(length as i32)?;
                let count = env
                    .call_method(
                        self.input.as_obj(),
                        "read",
                        "([B)I",
                        &[JValue::Object(&array)],
                    )?
                    .i()?;
                if count == -1 {
                    return Ok(Some(0));
                }
                if count <= 0 || count as usize > length {
                    return Ok(None);
                }
                let mut bytes = vec![0i8; count as usize];
                env.get_byte_array_region(&array, 0, &mut bytes)?;
                for (target, byte) in output.iter_mut().zip(bytes) {
                    *target = byte as u8;
                }
                Ok(Some(count as usize))
            });
            if env.exception_check().unwrap_or(true) {
                let _ = env.exception_clear();
                return None;
            }
            result.ok().flatten()
        })();
        result.ok_or_else(|| io::Error::other("Restore input failed"))
    }
}
impl Drop for JavaReader {
    fn drop(&mut self) {
        let success = (|| -> Option<()> {
            let mut env = self.vm.attach_current_thread().ok()?;
            let result = env.call_method(self.input.as_obj(), "retire", "()V", &[]);
            if env.exception_check().unwrap_or(true) {
                let _ = env.exception_clear();
                return None;
            }
            result.ok()?;
            Some(())
        })()
        .is_some();
        self.retired.store(success, Ordering::Release);
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_prepareRestore(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    secret: JCharArray<'_>,
    recovery: jboolean,
    input: JObject<'_>,
    directory: JString<'_>,
    limit: jlong,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        if limit <= 0 || input.is_null() {
            return None;
        }
        let directory = bounded_string(&mut env, &directory, 4096)?;
        if directory.len() > 4096 || !std::path::Path::new(&directory).is_absolute() {
            return None;
        }
        let length = env.get_array_length(&secret).ok()?;
        if length > 1024 {
            return None;
        }
        let mut chars = Zeroizing::new(vec![0u16; length as usize]);
        env.get_char_array_region(&secret, 0, &mut chars).ok()?;
        let mut secret = Zeroizing::new(String::with_capacity(chars.len() * 3));
        for scalar in char::decode_utf16(chars.iter().copied()) {
            secret.push(scalar.ok()?);
        }
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let control = {
            let mut entries = jobs().lock().ok()?;
            let entry = entries.get_mut(&token)?;
            if entry.host != handle || entry.running || entry.review.is_some() {
                return None;
            }
            entry.control.check().ok()?;
            entry.running = true;
            entry.control.clone()
        };
        let mut running = Running { token, keep: false };
        let retired = Arc::new(AtomicBool::new(false));
        let reader = JavaReader {
            vm: env.get_java_vm().ok()?,
            input: env.new_global_ref(input).ok()?,
            retired: Arc::clone(&retired),
        };
        let credential = if recovery != 0 {
            Credential::Recovery(&secret)
        } else {
            Credential::Password(&secret)
        };
        let review = host
            .prepare_restore(
                reader,
                credential,
                limit as u64,
                std::path::Path::new(&directory),
                &control,
            )
            .ok()?;
        if !retired.load(Ordering::Acquire) {
            return None;
        }
        control.check().ok()?;
        let summary = serde_json::to_string(review.summary()).ok()?;
        let result = env.new_string(summary).ok()?;
        {
            let mut entries = jobs().lock().ok()?;
            let entry = entries.get_mut(&token)?;
            entry.control.check().ok()?;
            entry.review = Some(review);
        }
        running.keep = true;
        Some(result.into_raw())
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_publishRestore(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    destination: JString<'_>,
    proof: JObject<'_>,
    confirmed: jboolean,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let path = bounded_string(&mut env, &destination, 4096)?;
        if path.len() > 4096 || !std::path::Path::new(&path).is_absolute() || proof.is_null() {
            return None;
        }
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let mut auth = JavaAuth {
            vm: env.get_java_vm().ok()?,
            proof: env.new_global_ref(proof).ok()?,
        };
        let (review, control) = {
            let mut entries = jobs().lock().ok()?;
            let entry = entries.get_mut(&token)?;
            if entry.host != handle || entry.running {
                return None;
            }
            entry.control.check().ok()?;
            let review = entry.review.take()?;
            entry.running = true;
            (review, entry.control.clone())
        };
        let _running = Running { token, keep: false };
        let publication = host
            .publish_restore(
                review,
                std::path::Path::new(&path),
                &mut auth,
                confirmed != 0,
                &control,
            )
            .ok()?;
        Some(
            env.new_string(serde_json::to_string(&publication).ok()?)
                .ok()?
                .into_raw(),
        )
    }));
    match result {
        Ok(Some(value)) => value,
        _ => {
            fail(&mut env);
            std::ptr::null_mut()
        }
    }
}
