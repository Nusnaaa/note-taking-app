//! Optional-worker read-only depot discovery with explicit cancellation ownership.
use super::*;
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_beginEngineRecovery(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    budget: jlong,
) -> jlong {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<i64> {
        if !(1..=3600000).contains(&budget) {
            return None;
        }
        let control = JobControl::new(Duration::from_millis(budget as u64)).ok()?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        insert(
            handle,
            control,
            Work::Recovery(host.engine_recovery_plan().ok()?),
        )
    }));
    match result {
        Ok(Some(value)) => value,
        _ => {
            clear_exception(&mut env);
            fail(&mut env);
            0
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_runEngineRecovery(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    root: JString<'_>,
    after: JString<'_>,
    limit: jint,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        if !(1..=128).contains(&limit) {
            return None;
        }
        let root = bounded_string(&mut env, &root, 4096)?;
        if root.is_empty() || root.contains('\0') {
            return None;
        }
        let after = if after.is_null() {
            None
        } else {
            Some(Id::parse(&bounded_string(&mut env, &after, 36)?).ok()?)
        };
        let (work, control, _running) = take(handle, token)?;
        let Work::Recovery(plan) = work else {
            return None;
        };
        let page = plan
            .scan(std::path::Path::new(&root), after, limit as usize, &control)
            .ok()?;
        let json = serde_json::to_string(&page).ok()?;
        if json.len() > MAX_JSON {
            return None;
        }
        Some(env.new_string(json).ok()?.into_raw())
    }));
    match result {
        Ok(Some(value)) => value,
        _ => {
            clear_exception(&mut env);
            fail(&mut env);
            std::ptr::null_mut()
        }
    }
}
