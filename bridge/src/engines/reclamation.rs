//! A native review owner survives between review and explicit confirmation.
use super::*;
fn optional_id(env: &mut JNIEnv<'_>, value: &JString<'_>) -> Option<Option<Id>> {
    if value.is_null() {
        Some(None)
    } else {
        Some(Some(Id::parse(&bounded_string(env, value, 36)?).ok()?))
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_beginEngineReclamation(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    version: JString<'_>,
    resume: JString<'_>,
    budget: jlong,
) -> jlong {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<i64> {
        if !(1..=3600000).contains(&budget) {
            return None;
        }
        let version = Id::parse(&bounded_string(&mut env, &version, 36)?).ok()?;
        let resume = optional_id(&mut env, &resume)?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let control = JobControl::new(Duration::from_millis(budget as u64)).ok()?;
        insert(
            handle,
            control,
            Work::ReclamationPlan(host.engine_reclamation_plan(version, resume).ok()?),
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_engineReclamationActionId(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let id = {
            let entries = jobs().lock().ok()?;
            let entry = entries.get(&token).filter(|e| e.host == handle)?;
            entry.control.check().ok()?;
            let Some(Work::ReclamationPlan(plan)) = &entry.work else {
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_reviewEngineReclamation(
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
        let (work, control, running) = take(handle, token)?;
        let Work::ReclamationPlan(plan) = work else {
            return None;
        };
        let reviewed = plan.review(std::path::Path::new(&root), &control).ok()?;
        let summary = serde_json::to_string(reviewed.summary()).ok()?;
        if summary.len() > MAX_JSON {
            return None;
        }
        running
            .restore(handle, Work::ReclamationReviewed(Box::new(reviewed)))
            .ok()?;
        Some(env.new_string(summary).ok()?.into_raw())
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_runEngineReclamation(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    confirmed: jboolean,
    binding: jboolean,
    retirement: jboolean,
    symlinks: jboolean,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let (work, control, _running) = take(handle, token)?;
        let Work::ReclamationReviewed(review) = work else {
            return None;
        };
        let consent = stem_core::store::ReclamationConsent {
            confirmed: confirmed == 1,
            reviewed_unknown_binding: binding == 1,
            reviewed_failed_retirement: retirement == 1,
            reviewed_symlinks: symlinks == 1,
        };
        let prepared = review.prepare(consent, &control).ok()?;
        host.authorise_engine_reclamation(&prepared, &control)
            .ok()?;
        let removed = prepared.remove(&control).ok()?;
        let receipt = host.complete_engine_reclamation(&removed, &control).ok()?;
        // Do not discard known SQL success if cancellation races its reply.
        Some(
            env.new_string(serde_json::to_string(&receipt).ok()?)
                .ok()?
                .into_raw(),
        )
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_engineReclamations(
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
            serde_json::to_string(&host.engine_reclamations(after, limit as usize).ok()?).ok()?;
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_engineReclamation(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    id: JString<'_>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let id = Id::parse(&bounded_string(&mut env, &id, 36)?).ok()?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        Some(
            env.new_string(serde_json::to_string(&host.engine_reclamation(id).ok()?).ok()?)
                .ok()?
                .into_raw(),
        )
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
