//! Trusted worker-only native subset export. Opaque reviews are host-bound,
//! one-shot, and retired on cancellation/close. Registry locks exclude callbacks.
use super::{hosts, JavaAuth};
use jni::{
    objects::{JCharArray, JClass, JObject, JString, JValue},
    sys::{jboolean, jlong, jobject},
    JNIEnv,
};
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicI64, Arc, Mutex, OnceLock},
    time::Duration,
};
use stem_core::{
    host::{NativeExportReview, NativeExportSelection},
    portable::ExportProtection,
    processing::JobControl,
};
use zeroize::Zeroizing;
struct Entry {
    host: i64,
    review: Option<NativeExportReview>,
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
        "Native export operation failed",
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
struct Running(i64);
impl Drop for Running {
    fn drop(&mut self) {
        if let Ok(mut e) = jobs().lock() {
            e.remove(&self.0);
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_reviewNativeExport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    selection: JString<'_>,
    ink: jboolean,
) -> jobject {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jobject> {
        let text = super::bounded_string(&mut env, &selection, 1024 * 1024)?;
        let selection = serde_json::from_str::<NativeExportSelection>(&text).ok()?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let review = host.prepare_native_export(selection, ink != 0).ok()?;
        let summary = env
            .new_string(serde_json::to_string(review.summary()).ok()?)
            .ok()?;
        let token = super::allocate_handle(&NEXT)?;
        {
            // Closing removes the host before retiring its jobs. Hold that same
            // ordering while publishing so a late review cannot orphan a slot.
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
                    review: Some(review),
                    control: None,
                    running: false,
                },
            );
        }
        match env.new_object(
            "org/stemnotebook/backend/NativeBridge$NativeExport",
            "(JJLjava/lang/String;)V",
            &[
                JValue::Long(handle),
                JValue::Long(token),
                JValue::Object(&summary),
            ],
        ) {
            Ok(object) => Some(object.into_raw()),
            Err(_) => {
                jobs().lock().ok()?.remove(&token);
                None
            }
        }
    }));
    match result {
        Ok(Some(output)) => output,
        _ => {
            fail(&mut env);
            std::ptr::null_mut()
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_runNativeExport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    password: JCharArray<'_>,
    proof: JObject<'_>,
    protected: jboolean,
    omissions: jboolean,
    budget: jlong,
) -> jobject {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jobject> {
        if proof.is_null() || !(1..=3_600_000).contains(&budget) {
            return None;
        }
        let secret = if password.is_null() {
            None
        } else {
            let len = env.get_array_length(&password).ok()?;
            if len > 1024 {
                return None;
            }
            let mut chars = Zeroizing::new(vec![0u16; len as usize]);
            env.get_char_array_region(&password, 0, &mut chars).ok()?;
            let mut secret = Zeroizing::new(String::with_capacity(chars.len() * 3));
            for scalar in char::decode_utf16(chars.iter().copied()) {
                secret.push(scalar.ok()?);
            }
            Some(secret)
        };
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let mut auth = JavaAuth {
            vm: env.get_java_vm().ok()?,
            proof: env.new_global_ref(proof).ok()?,
        };
        let control = JobControl::new(Duration::from_millis(budget as u64)).ok()?;
        let review = {
            let mut entries = jobs().lock().ok()?;
            let e = entries.get_mut(&token)?;
            if e.host != handle || e.running {
                return None;
            }
            let review = e.review.take()?;
            e.control = Some(control.clone());
            e.running = true;
            review
        };
        let _running = Running(token);
        let protection = match &secret {
            Some(secret) => ExportProtection::Encrypted { password: secret },
            None => ExportProtection::Plaintext {
                acknowledge_protected: protected != 0,
            },
        };
        let completed = host
            .encode_native_export(review, protection, &mut auth, omissions != 0, &control)
            .ok()?;
        control.check().ok()?;
        let output = host.finish_native_export(completed).ok()?;
        control.check().ok()?;
        let bytes = env.byte_array_from_slice(&output.bytes).ok()?;
        let key = if let Some(key) = output.recovery_key {
            let chars = Zeroizing::new(key.encode_utf16().collect::<Vec<_>>());
            let array = env.new_char_array(i32::try_from(chars.len()).ok()?).ok()?;
            env.set_char_array_region(&array, 0, &chars).ok()?;
            JObject::from(array)
        } else {
            JObject::null()
        };
        control.check().ok()?;
        Some(
            env.new_object(
                "org/stemnotebook/backend/NativeBridge$NativeExportData",
                "([B[C)V",
                &[JValue::Object(&bytes), JValue::Object(&key)],
            )
            .ok()?
            .into_raw(),
        )
    }));
    match result {
        Ok(Some(output)) => output,
        _ => {
            fail(&mut env);
            std::ptr::null_mut()
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_releaseNativeExport(
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
        if let Some(control) = &e.control {
            control.cancel();
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
