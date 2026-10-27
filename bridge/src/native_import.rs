//! Native import transport. Operation identity precedes publication; committed
//! receipts remain queryable after any lost JNI response.
use super::{hosts, JavaAuth};
use jni::{
    objects::{JByteArray, JCharArray, JClass, JObject, JString, JValue},
    sys::{jboolean, jlong, jobject, jstring},
    JNIEnv,
};
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicI64, Arc, Mutex, OnceLock},
    time::Duration,
};
use stem_core::{
    crypto::Credential,
    host::{NativeImportReview, NativeImportSource},
    portable::NATIVE_IMPORT_MAX_BYTES,
    processing::JobControl,
};
enum Phase {
    Source(Option<NativeImportSource>),
    Review(Option<Box<NativeImportReview>>),
}
struct Entry {
    host: i64,
    phase: Phase,
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
        "Native import operation failed; reconcile operation receipt before retrying",
    );
}
pub(super) fn close(host: i64) {
    if let Ok(mut entries) = jobs().lock() {
        entries.retain(|_, e| {
            if e.host != host {
                return true;
            }
            if let Some(c) = &e.control {
                c.cancel();
            }
            e.running
        });
    }
}
struct Running {
    token: i64,
    keep: bool,
}
impl Drop for Running {
    fn drop(&mut self) {
        if !self.keep {
            if let Ok(mut entries) = jobs().lock() {
                entries.remove(&self.token);
            }
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_beginNativeImport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) -> jobject {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jobject> {
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let source = host.native_import_source().ok()?;
        let operation = env.new_string(source.operation().to_string()).ok()?;
        let token = super::allocate_handle(&NEXT)?;
        {
            let owners = hosts().lock().ok()?;
            if !owners
                .get(&handle)
                .is_some_and(|owner| Arc::ptr_eq(owner, &host))
            {
                return None;
            }
            let mut entries = jobs().lock().ok()?;
            if entries.len() >= 128 {
                return None;
            }
            entries.insert(
                token,
                Entry {
                    host: handle,
                    phase: Phase::Source(Some(source)),
                    control: None,
                    running: false,
                },
            );
        }
        let mut cleanup = Running { token, keep: false };
        let object = env
            .new_object(
                "org/stemnotebook/backend/NativeBridge$NativeImport",
                "(JJLjava/lang/String;)V",
                &[
                    JValue::Long(handle),
                    JValue::Long(token),
                    JValue::Object(&operation),
                ],
            )
            .ok()?;
        cleanup.keep = true;
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_prepareNativeImport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    bytes: JByteArray<'_>,
    credential: JCharArray<'_>,
    recovery: jboolean,
    budget: jlong,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        if !(1..=3_600_000).contains(&budget)
            || bytes.is_null()
            || env.get_array_length(&bytes).ok()? as usize > NATIVE_IMPORT_MAX_BYTES
        {
            return None;
        }
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let control = JobControl::new(Duration::from_millis(budget as u64)).ok()?;
        let source = {
            let mut entries = jobs().lock().ok()?;
            let e = entries.get_mut(&token)?;
            if e.host != handle || e.running {
                return None;
            }
            let Phase::Source(source) = &mut e.phase else {
                return None;
            };
            let source = source.take()?;
            e.running = true;
            e.control = Some(control.clone());
            source
        };
        let mut running = Running { token, keep: false };
        let bytes = zeroize::Zeroizing::new(env.convert_byte_array(&bytes).ok()?);
        let password = if credential.is_null() {
            if recovery != 0 {
                return None;
            }
            None
        } else {
            let len = env.get_array_length(&credential).ok()?;
            if !(1..=4096).contains(&len) {
                return None;
            }
            let mut chars = zeroize::Zeroizing::new(vec![0u16; len as usize]);
            env.get_char_array_region(&credential, 0, &mut chars).ok()?;
            let mut value = zeroize::Zeroizing::new(String::with_capacity(chars.len() * 3));
            for scalar in char::decode_utf16(chars.iter().copied()) {
                value.push(scalar.ok()?);
            }
            Some(value)
        };
        let credential = password.as_ref().map(|p| {
            if recovery != 0 {
                Credential::Recovery(p)
            } else {
                Credential::Password(p)
            }
        });
        let review = host
            .prepare_native_import(source, &bytes, credential, &control)
            .ok()?;
        let summary = env
            .new_string(serde_json::to_string(review.summary().ok()?).ok()?)
            .ok()?;
        {
            let owners = hosts().lock().ok()?;
            if !owners
                .get(&handle)
                .is_some_and(|owner| Arc::ptr_eq(owner, &host))
            {
                return None;
            }
            control.check().ok()?;
            let mut entries = jobs().lock().ok()?;
            let e = entries.get_mut(&token)?;
            if e.host != handle || !e.running {
                return None;
            }
            control.check().ok()?;
            e.phase = Phase::Review(Some(Box::new(review)));
            e.running = false;
        }
        running.keep = true;
        Some(summary.into_raw())
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_publishNativeImport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    proof: JObject<'_>,
    confirmed: jboolean,
    acknowledge: jboolean,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        if proof.is_null() {
            return None;
        }
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let mut auth = JavaAuth {
            vm: env.get_java_vm().ok()?,
            proof: env.new_global_ref(proof).ok()?,
        };
        let review = {
            let mut entries = jobs().lock().ok()?;
            let e = entries.get_mut(&token)?;
            if e.host != handle || e.running {
                return None;
            }
            let Phase::Review(review) = &mut e.phase else {
                return None;
            };
            let review = review.take()?;
            e.running = true;
            review
        };
        let _running = Running { token, keep: false };
        let receipt = host
            .publish_native_import(*review, &mut auth, confirmed != 0, acknowledge != 0)
            .ok()?;
        // No cancellation/host-close checks after the known commit. A failed
        // allocation still leaves the durable receipt available for reconciliation.
        Some(
            env.new_string(serde_json::to_string(&receipt).ok()?)
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
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_nativeImportReceipt(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    operation: JString<'_>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let operation =
            stem_core::Id::parse(&super::bounded_string(&mut env, &operation, 36)?).ok()?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let receipt = host.native_import_receipt(operation).ok()?;
        Some(
            env.new_string(serde_json::to_string(&receipt).ok()?)
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
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_releaseNativeImport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
) {
    let result = std::panic::catch_unwind(|| -> Option<()> {
        let mut entries = jobs().lock().ok()?;
        let Some(e) = entries.get_mut(&token) else {
            return Some(());
        };
        if e.host != handle {
            return None;
        }
        if let Some(c) = &e.control {
            c.cancel();
        }
        if !e.running {
            entries.remove(&token);
        }
        Some(())
    });
    if !matches!(result, Ok(Some(()))) {
        fail(&mut env);
    }
}
