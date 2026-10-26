//! Logical version removal; resource files are retained for explicit lease-safe
//! cleanup. SDK owners retire through the Host lane, never on its writer.
use super::*;
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_beginEngineRemoval(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    version: JString<'_>,
    budget: jlong,
) -> jlong {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<i64> {
        if !(1..=3600000).contains(&budget) {
            return None;
        }
        let control = JobControl::new(Duration::from_millis(budget as u64)).ok()?;
        let version = Id::parse(&bounded_string(&mut env, &version, 36)?).ok()?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        insert(
            handle,
            control,
            Work::Removal(host.engine_removal_plan(version).ok()?),
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_runEngineRemoval(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<()> {
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let (work, control, _running) = take(handle, token)?;
        let Work::Removal(plan) = work else {
            return None;
        };
        host.remove_prepared_engine(&plan, &control).ok()
    }));
    if !matches!(result, Ok(Some(()))) {
        clear_exception(&mut env);
        fail(&mut env);
    }
}
