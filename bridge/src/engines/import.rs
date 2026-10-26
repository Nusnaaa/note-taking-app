//! Resumable local resource input. Input objects never enter a writer mailbox.
use super::*;
use jni::sys::jobject;
use std::io::{self, Read};

struct JavaInput {
    vm: JavaVM,
    object: GlobalRef,
    class: GlobalRef,
    control: JobControl,
}
impl Read for JavaInput {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if output.len() > 65536 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let result = (|| -> Result<usize> {
            self.control.check()?;
            let token = CallbackControl::new(&self.control)?;
            let mut env = self
                .vm
                .attach_current_thread()
                .map_err(|_| Error::WorkerFailed)?;
            let result = frame(&mut env, 8, |env| {
                let control = control_object(env, &self.class, token.0)?;
                let array = env
                    .new_byte_array(output.len() as i32)
                    .map_err(|_| Error::WorkerFailed)?;
                let called = env.call_method(
                    self.object.as_obj(),
                    "read",
                    "([BLorg/stemnotebook/backend/NativeBridge$EngineControl;)I",
                    &[JValue::Object(&array), JValue::Object(&control)],
                );
                let copied = (|| -> Result<usize> {
                    let count = jni_result(env, called)?
                        .i()
                        .map_err(|_| Error::WorkerFailed)?;
                    if count == -1 {
                        return Ok(0);
                    }
                    if count <= 0 || count as usize > output.len() {
                        return Err(Error::Integrity);
                    }
                    let bytes = zeroize::Zeroizing::new(
                        env.convert_byte_array(&array)
                            .map_err(|_| Error::WorkerFailed)?,
                    );
                    output[..count as usize].copy_from_slice(&bytes[..count as usize]);
                    Ok(count as usize)
                })();
                clear_exception(env);
                let zeros = [0i8; 65536];
                let wiped = env.set_byte_array_region(&array, 0, &zeros[..output.len()]);
                jni_result(env, wiped)?;
                self.control.check()?;
                copied
            });
            clear_exception(&mut env);
            result
        })();
        result.map_err(|_| io::Error::other("engine input failed"))
    }
}
impl Drop for JavaInput {
    fn drop(&mut self) {
        let retired = self.vm.attach_current_thread().ok().is_some_and(|mut env| {
            let result = env.call_method(self.object.as_obj(), "retire", "()V", &[]);
            jni_result(&mut env, result).is_ok()
        });
        if !retired && !std::thread::panicking() {
            panic!("engine input retirement failed");
        }
    }
}
fn open_input(
    vm: &JavaVM,
    factory: &GlobalRef,
    class: &GlobalRef,
    control: &JobControl,
) -> Result<JavaInput> {
    control.check()?;
    let token = CallbackControl::new(control)?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|_| Error::WorkerFailed)?;
    frame(&mut env, 8, |env| {
        let owner_vm = env.get_java_vm().map_err(|_| Error::WorkerFailed)?;
        let linked = control_object(env, class, token.0)?;
        let called = env.call_method(factory.as_obj(), "open", "(Lorg/stemnotebook/backend/NativeBridge$EngineControl;)Lorg/stemnotebook/backend/NativeBridge$EngineInput;", &[JValue::Object(&linked)]);
        let object = jni_result(env, called)?
            .l()
            .map_err(|_| Error::WorkerFailed)?;
        if object.is_null() {
            return Err(Error::WorkerFailed);
        }
        let object = match env.new_global_ref(&object) {
            Ok(owner) => owner,
            Err(_) => {
                clear_exception(env);
                let retired = env.call_method(&object, "retire", "()V", &[]);
                let _ = jni_result(env, retired);
                return Err(Error::WorkerFailed);
            }
        };
        let input = JavaInput {
            vm: owner_vm,
            object,
            class: class.clone(),
            control: control.clone(),
        };
        control.check()?;
        Ok(input)
    })
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_beginEngineImport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    descriptor: JString<'_>,
    total: jlong,
    confirmed: jboolean,
    budget: jlong,
) -> jlong {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<i64> {
        if total <= 0 || !(1..=3600000).contains(&budget) {
            return None;
        }
        let descriptor =
            serde_json::from_str(&bounded_string(&mut env, &descriptor, 60 * 1024)?).ok()?;
        let control = JobControl::new(Duration::from_millis(budget as u64)).ok()?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let plan = host
            .engine_import_plan(descriptor, total as u64, confirmed != 0)
            .ok()?;
        insert(handle, control, Work::ImportPlan(plan))
    }));
    match result {
        Ok(Some(id)) => id,
        _ => {
            clear_exception(&mut env);
            fail(&mut env);
            0
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_startEngineImport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    root: JString<'_>,
    resume: JString<'_>,
) -> jobject {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jobject> {
        let root = bounded_string(&mut env, &root, 4096)?;
        if !std::path::Path::new(&root).is_absolute() {
            return None;
        }
        let resume = if resume.is_null() {
            None
        } else {
            Some(Id::parse(&bounded_string(&mut env, &resume, 36)?).ok()?)
        };
        let (work, control, running) = take(handle, token)?;
        let Work::ImportPlan(plan) = work else {
            return None;
        };
        let transfer = plan
            .start(std::path::Path::new(&root), resume, &control)
            .ok()?;
        let version = env.new_string(transfer.id().to_string()).ok()?;
        let committed = transfer.committed_bytes();
        running
            .restore(handle, Work::ImportTransfer(transfer))
            .ok()?;
        Some(
            env.new_object(
                "org/stemnotebook/backend/NativeBridge$EngineImport",
                "(JJLjava/lang/String;J)V",
                &[
                    JValue::Long(handle),
                    JValue::Long(token),
                    JValue::Object(&version),
                    JValue::Long(committed as i64),
                ],
            )
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_copyEngineImport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    input: JObject<'_>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let (work, control, _running) = take(handle, token)?;
        let Work::ImportTransfer(transfer) = work else {
            return None;
        };
        let (vm, object, class) = factory(&mut env, input).ok()?;
        let prepared = transfer
            .prepare_with(&control, move |linked| {
                open_input(&vm, &object, &class, linked)
            })
            .ok()?;
        let version = host.commit_engine_import(&prepared).ok()?;
        Some(
            env.new_string(serde_json::to_string(&version).ok()?)
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
