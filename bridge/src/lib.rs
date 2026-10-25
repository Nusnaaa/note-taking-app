//! The only JNI boundary. No raw Rust pointer crosses it; opaque, non-reused handles
//! refer to a locked registry. Panic containment and bounded requests fail closed.
mod backup;
mod restore;
use jni::{
    objects::{JClass, JString},
    sys::{jlong, jstring},
    JNIEnv,
};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicI64, Ordering},
    sync::{Arc, Mutex, OnceLock},
};
use stem_core::host::Host;
static HOSTS: OnceLock<Mutex<BTreeMap<i64, Arc<Host>>>> = OnceLock::new();
static NEXT: AtomicI64 = AtomicI64::new(1);
fn hosts() -> &'static Mutex<BTreeMap<i64, Arc<Host>>> {
    HOSTS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn allocate_handle(next: &AtomicI64) -> Option<i64> {
    next.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
        (id > 0).then(|| id.checked_add(1)).flatten()
    })
    .ok()
}
fn fail(env: &mut JNIEnv<'_>) {
    let _ = env.throw_new(
        "java/lang/IllegalStateException",
        "Native notebook operation failed",
    );
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_open(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    path: JString<'_>,
) -> jlong {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<i64> {
        let path = bounded_string(&mut env, &path, 4096)?;
        if path.len() > 4096 || !std::path::Path::new(&path).is_absolute() {
            return None;
        }
        // Allocate before opening SQLite so exhausted handles have no disk side effects.
        let id = allocate_handle(&NEXT)?;
        let host = Host::open(path.into(), 64).ok()?;
        hosts().lock().ok()?.insert(id, Arc::new(host));
        Some(id)
    }));
    match result {
        Ok(Some(id)) => id,
        _ => {
            fail(&mut env);
            0
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_captureLibraryIdentity(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let identity = host.capture_library_identity().ok()?;
        Some(env.new_string(identity).ok()?.into_raw())
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_request(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    request: JString<'_>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        // Bound Java UTF-16 input before requesting a native copy.
        if env
            .call_method(&request, "length", "()I", &[])
            .ok()?
            .i()
            .ok()?
            > 8 * 1024 * 1024
        {
            return None;
        }
        let request: String = env.get_string(&request).ok()?.into();
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let response = host.request(&request).ok()?;
        let json = serde_json::to_string(&response).ok()?;
        Some(env.new_string(json).ok()?.into_raw())
    }));
    match result {
        Ok(Some(s)) => s,
        _ => {
            fail(&mut env);
            std::ptr::null_mut()
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_close(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) {
    let result = std::panic::catch_unwind(|| {
        let host = hosts().lock().ok()?.remove(&handle)?;
        backup::close(handle);
        restore::close(handle);
        host.close().ok()?;
        Some(())
    });
    if !matches!(result, Ok(Some(()))) {
        fail(&mut env)
    }
}

struct JavaAuth {
    vm: jni::JavaVM,
    proof: jni::objects::GlobalRef,
}
impl stem_core::security::Authenticator for JavaAuth {
    fn verify(
        &mut self,
        challenge: stem_core::Id,
        _: &std::collections::BTreeSet<stem_core::security::Scope>,
    ) -> bool {
        let Ok(mut env) = self.vm.attach_current_thread() else {
            return false;
        };
        let Ok(value) = env.new_string(challenge.to_string()) else {
            return false;
        };
        let result = env
            .call_method(
                self.proof.as_obj(),
                "verify",
                "(Ljava/lang/String;)Z",
                &[jni::objects::JValue::Object(&value)],
            )
            .and_then(|r| r.z());
        if env.exception_check().unwrap_or(true) {
            let _ = env.exception_clear();
            return false;
        }
        result.unwrap_or(false)
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_authenticate(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    scopes: JString<'_>,
    proof: jni::objects::JObject<'_>,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<()> {
        if env
            .call_method(&scopes, "length", "()I", &[])
            .ok()?
            .i()
            .ok()?
            > 65536
        {
            return None;
        }
        let json: String = env.get_string(&scopes).ok()?.into();
        let scopes: std::collections::BTreeSet<stem_core::security::Scope> =
            serde_json::from_str(&json).ok()?;
        if scopes.len() > 1024 {
            return None;
        }
        let verifier = JavaAuth {
            vm: env.get_java_vm().ok()?,
            proof: env.new_global_ref(proof).ok()?,
        };
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        host.authenticate(scopes, Box::new(verifier)).ok()
    }));
    if !matches!(result, Ok(Some(()))) {
        fail(&mut env)
    }
}

struct JavaPdfGrant {
    vm: jni::JavaVM,
    proof: jni::objects::GlobalRef,
}
impl stem_core::host::PdfGrantVerifier for JavaPdfGrant {
    fn verify(
        &mut self,
        challenge: stem_core::Id,
        target: &stem_core::sources::PdfSelectionTarget,
        locator: &str,
        revision: &str,
    ) -> bool {
        let Ok(mut env) = self.vm.attach_current_thread() else {
            return false;
        };
        let Ok(target) = serde_json::to_string(target) else {
            return false;
        };
        let values = [
            challenge.to_string(),
            target,
            locator.to_owned(),
            revision.to_owned(),
        ];
        let strings = values
            .iter()
            .map(|s| env.new_string(s))
            .collect::<std::result::Result<Vec<_>, _>>();
        let Ok(strings) = strings else {
            return false;
        };
        let arguments = strings
            .iter()
            .map(|s| jni::objects::JValue::Object(s.as_ref()))
            .collect::<Vec<_>>();
        let result = env
            .call_method(
                self.proof.as_obj(),
                "verify",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z",
                &arguments,
            )
            .and_then(|r| r.z());
        if env.exception_check().unwrap_or(true) {
            let _ = env.exception_clear();
            return false;
        }
        result.unwrap_or(false)
    }
}
fn bounded_string(env: &mut JNIEnv<'_>, value: &JString<'_>, maximum: i32) -> Option<String> {
    if env
        .call_method(value, "length", "()I", &[])
        .ok()?
        .i()
        .ok()?
        > maximum
    {
        return None;
    }
    Some(env.get_string(value).ok()?.into())
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_preparePdfSelection(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    target: JString<'_>,
    locator: JString<'_>,
    revision: JString<'_>,
    proof: jni::objects::JObject<'_>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let target = bounded_string(&mut env, &target, 1024)?;
        let target: stem_core::sources::PdfSelectionTarget = serde_json::from_str(&target).ok()?;
        let locator = bounded_string(&mut env, &locator, 4096)?;
        let revision = bounded_string(&mut env, &revision, 64)?;
        let verifier = JavaPdfGrant {
            vm: env.get_java_vm().ok()?,
            proof: env.new_global_ref(proof).ok()?,
        };
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let review = host
            .prepare_pdf_selection(target, locator, revision, Box::new(verifier))
            .ok()?;
        Some(
            env.new_string(serde_json::to_string(&review).ok()?)
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_finishPdfSelection(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    review: JString<'_>,
    confirmed: jni::sys::jboolean,
    proof: jni::objects::JObject<'_>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let review = stem_core::Id::parse(&bounded_string(&mut env, &review, 36)?).ok()?;
        let verifier = JavaPdfGrant {
            vm: env.get_java_vm().ok()?,
            proof: env.new_global_ref(proof).ok()?,
        };
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let source = host
            .finish_pdf_selection(review, confirmed != 0, Box::new(verifier))
            .ok()?;
        Some(
            env.new_string(serde_json::to_string(&source).ok()?)
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

/// A trusted input controller may suppress optional recognition without waiting
/// for SQLite or invoking an engine. Model code is never given this boundary.
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_setCapturePriority(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    session: JString<'_>,
    active: jni::sys::jboolean,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<()> {
        if env
            .call_method(&session, "length", "()I", &[])
            .ok()?
            .i()
            .ok()?
            != 36
        {
            return None;
        }
        let session: String = env.get_string(&session).ok()?.into();
        let session = stem_core::Id::parse(&session).ok()?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        host.set_capture_priority(session, active != 0).ok()?;
        Some(())
    }));
    if !matches!(result, Ok(Some(()))) {
        fail(&mut env);
    }
}

#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_recognitionStatus(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let status = serde_json::to_string(&host.recognition_status()).ok()?;
        Some(env.new_string(status).ok()?.into_raw())
    }));
    match result {
        Ok(Some(value)) => value,
        _ => {
            fail(&mut env);
            std::ptr::null_mut()
        }
    }
}

#[cfg(test)]
mod handle_tests {
    use super::*;

    #[test]
    fn exhaustion_never_wraps_or_reuses_a_handle() {
        let next = AtomicI64::new(i64::MAX - 1);
        assert_eq!(allocate_handle(&next), Some(i64::MAX - 1));
        assert_eq!(allocate_handle(&next), None);
        assert_eq!(allocate_handle(&next), None);
        assert_eq!(next.load(Ordering::Relaxed), i64::MAX);
        assert_eq!(allocate_handle(&AtomicI64::new(0)), None);
        assert_eq!(allocate_handle(&AtomicI64::new(-1)), None);
    }
}
