//! Explicit unpublished-import discard; stable receipt identity precedes file work.
use super::*;
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_beginEngineDiscard(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    version: JString<'_>,
    confirmed: jboolean,
    resume: JString<'_>,
    budget: jlong,
) -> jlong {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<i64> {
        if !(1..=3600000).contains(&budget) || confirmed != 1 {
            return None;
        }
        let version = Id::parse(&bounded_string(&mut env, &version, 36)?).ok()?;
        let resume = optional_id(&mut env, &resume)?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let control = JobControl::new(Duration::from_millis(budget as u64)).ok()?;
        insert(
            handle,
            control,
            Work::Discard(host.engine_discard_plan(version, true, resume).ok()?),
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
fn optional_id(env: &mut JNIEnv<'_>, value: &JString<'_>) -> Option<Option<Id>> {
    if value.is_null() {
        Some(None)
    } else {
        Some(Some(Id::parse(&bounded_string(env, value, 36)?).ok()?))
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_engineDiscardActionId(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let id = {
            let entries = jobs().lock().ok()?;
            let entry = entries.get(&token).filter(|entry| entry.host == handle)?;
            entry.control.check().ok()?;
            let Some(Work::Discard(plan)) = &entry.work else {
                return None;
            };
            plan.action_id()
        };
        Some(env.new_string(id.to_string()).ok()?.into_raw())
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
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_runEngineDiscard(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    root: JString<'_>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let root = bounded_string(&mut env, &root, 4096)?;
        if root.is_empty() || root.contains('\0') {
            return None;
        }
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let (work, control, _running) = take(handle, token)?;
        let Work::Discard(plan) = work else {
            return None;
        };
        let prepared = plan.prepare(std::path::Path::new(&root), &control).ok()?;
        host.authorise_engine_discard(&prepared, &control).ok()?;
        let removed = prepared.remove(&control).ok()?;
        let receipt = host.complete_engine_discard(&removed, &control).ok()?;
        // Preserve a known Completed receipt through cancellation after commit.
        let json = serde_json::to_string(&receipt).ok()?;
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
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_engineResourceActions(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    after: JString<'_>,
    limit: jint,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        if !(1..=128).contains(&limit) {
            return None;
        }
        let after = optional_id(&mut env, &after)?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let json =
            serde_json::to_string(&host.engine_resource_actions(after, limit as usize).ok()?)
                .ok()?;
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
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_engineResourceAction(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    id: JString<'_>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let id = Id::parse(&bounded_string(&mut env, &id, 36)?).ok()?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let json = serde_json::to_string(&host.engine_resource_action(id).ok()?).ok()?;
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
