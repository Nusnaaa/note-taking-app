//! The only JNI boundary. No raw Rust pointer crosses it; opaque, non-reused handles
//! refer to a locked registry. Panic containment and bounded requests fail closed.
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
        let path: String = env.get_string(&path).ok()?.into();
        if path.len() > 4096 || !std::path::Path::new(&path).is_absolute() {
            return None;
        }
        let host = Host::open(path.into(), 64).ok()?;
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        if id <= 0 {
            return None;
        }
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
        drop(host);
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
