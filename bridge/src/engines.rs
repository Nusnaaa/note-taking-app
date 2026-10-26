//! Trusted worker-only compiled provider boundary. Never supplied by notebook
//! JSON. SDK construction, inference and disposal occur outside registry locks.
mod import;
mod recovery;
use super::{allocate_handle, bounded_string, fail, hosts};
use jni::{
    objects::{GlobalRef, JByteArray, JClass, JObject, JString, JValue},
    sys::{jboolean, jint, jlong, jstring},
    JNIEnv, JavaVM,
};
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicI64, Arc, Mutex, OnceLock},
    time::Duration,
};
use stem_core::{
    host::{HostEngineActivationPlan, HostRecognitionWork},
    processing::{Descriptor, JobControl, Recogniser, RecognitionOutput, Task},
    store::InkSnapshot,
    Error, Id, Result,
};

const MAX_JSON: usize = 8 * 1024 * 1024;
static NEXT: AtomicI64 = AtomicI64::new(1);
static JOBS: OnceLock<Mutex<BTreeMap<i64, Entry>>> = OnceLock::new();
static CONTROLS: OnceLock<Mutex<BTreeMap<i64, JobControl>>> = OnceLock::new();
fn jobs() -> &'static Mutex<BTreeMap<i64, Entry>> {
    JOBS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn controls() -> &'static Mutex<BTreeMap<i64, JobControl>> {
    CONTROLS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
enum Work {
    Activation(HostEngineActivationPlan, u64),
    Recognition(HostRecognitionWork),
    Recovery(stem_core::host::HostEngineRecoveryPlan),
    ImportPlan(stem_core::host::HostEngineImportPlan),
    ImportTransfer(stem_core::host::HostEngineImportTransfer),
}
struct Entry {
    host: i64,
    control: JobControl,
    work: Option<Work>,
}
struct Running(i64, bool);
impl Drop for Running {
    fn drop(&mut self) {
        if self.1 {
            if let Ok(mut entries) = jobs().lock() {
                entries.remove(&self.0);
            }
        }
    }
}
impl Running {
    fn restore(mut self, host: i64, work: Work) -> Result<()> {
        let owners = hosts().lock().map_err(|_| Error::WorkerFailed)?;
        if !owners.contains_key(&host) {
            return Err(Error::Cancelled);
        }
        let mut entries = jobs().lock().map_err(|_| Error::WorkerFailed)?;
        let entry = entries
            .get_mut(&self.0)
            .filter(|e| e.host == host)
            .ok_or(Error::Cancelled)?;
        entry.control.check()?;
        if entry.work.is_some() {
            return Err(Error::Conflict);
        }
        entry.work = Some(work);
        self.1 = false;
        Ok(())
    }
}
struct CallbackControl(i64);
impl CallbackControl {
    fn new(control: &JobControl) -> Result<Self> {
        let id = allocate_handle(&NEXT).ok_or(Error::ResourceLimit)?;
        controls()
            .lock()
            .map_err(|_| Error::WorkerFailed)?
            .insert(id, control.clone());
        Ok(Self(id))
    }
}
impl Drop for CallbackControl {
    fn drop(&mut self) {
        if let Ok(mut map) = controls().lock() {
            map.remove(&self.0);
        }
    }
}
fn frame<T>(
    env: &mut JNIEnv<'_>,
    capacity: i32,
    call: impl FnOnce(&mut JNIEnv<'_>) -> Result<T>,
) -> Result<T> {
    env.with_local_frame(capacity, |env| Ok::<_, jni::errors::Error>(call(env)))
        .map_err(|_| Error::WorkerFailed)?
}
fn clear_exception(env: &mut JNIEnv<'_>) -> bool {
    if env.exception_check().unwrap_or(true) {
        let _ = env.exception_clear();
        true
    } else {
        false
    }
}
fn jni_result<T>(env: &mut JNIEnv<'_>, result: jni::errors::Result<T>) -> Result<T> {
    let exception = clear_exception(env);
    if exception {
        Err(Error::WorkerFailed)
    } else {
        result.map_err(|_| Error::WorkerFailed)
    }
}
fn control_object<'a>(env: &mut JNIEnv<'a>, class: &GlobalRef, ticket: i64) -> Result<JObject<'a>> {
    let class: &JClass<'_> = class.as_obj().into();
    let result = env.new_object(class, "(J)V", &[JValue::Long(ticket)]);
    jni_result(env, result)
}
struct JavaRecogniser {
    vm: JavaVM,
    object: GlobalRef,
    control_class: GlobalRef,
    activation: JobControl,
}
impl Recogniser for JavaRecogniser {
    fn probe(&self) -> Result<()> {
        self.activation.check()?;
        let ticket = CallbackControl::new(&self.activation)?;
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|_| Error::WorkerFailed)?;
        frame(&mut env, 8, |env| -> Result<()> {
            let control = control_object(env, &self.control_class, ticket.0)?;
            let result = env.call_method(
                self.object.as_obj(),
                "probe",
                "(Lorg/stemnotebook/backend/NativeBridge$EngineControl;)V",
                &[JValue::Object(&control)],
            );
            jni_result(env, result)?;
            self.activation.check()
        })
    }
    fn recognise(
        &self,
        ink: &InkSnapshot,
        task: Task,
        control: &JobControl,
    ) -> Result<RecognitionOutput> {
        control.check()?;
        let ink = serde_json::to_string(ink)?;
        if ink.len() > MAX_JSON {
            return Err(Error::ResourceLimit);
        }
        let task = match task {
            Task::Maths => "Maths",
            Task::Prose => "Prose",
            Task::Numeric => "Numeric",
            Task::Code => "Code",
        };
        let ticket = CallbackControl::new(control)?;
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|_| Error::WorkerFailed)?;
        frame(&mut env, 12, |env| -> Result<RecognitionOutput> {
            let control_object = control_object(env, &self.control_class, ticket.0)?;
            let ink = env.new_string(ink).map_err(|_| Error::WorkerFailed)?;
            let task = env.new_string(task).map_err(|_| Error::WorkerFailed)?;
            let result = env.call_method(self.object.as_obj(), "recognise", "(Ljava/lang/String;Ljava/lang/String;Lorg/stemnotebook/backend/NativeBridge$EngineControl;)Ljava/lang/String;", &[JValue::Object(&ink), JValue::Object(&task), JValue::Object(&control_object)]);
            let output = jni_result(env, result)?
                .l()
                .map_err(|_| Error::WorkerFailed)?;
            if output.is_null() {
                return Err(Error::WorkerFailed);
            }
            let text = bounded_string(env, &JString::from(output), MAX_JSON as i32)
                .ok_or(Error::ResourceLimit)?;
            if text.len() > MAX_JSON {
                return Err(Error::ResourceLimit);
            }
            control.check()?;
            Ok(serde_json::from_str(&text)?)
        })
    }
}
impl Drop for JavaRecogniser {
    fn drop(&mut self) {
        let retired = self.vm.attach_current_thread().ok().is_some_and(|mut env| {
            let result = env.call_method(self.object.as_obj(), "retire", "()V", &[]);
            jni_result(&mut env, result).is_ok()
        });
        if !retired && !std::thread::panicking() {
            panic!("engine retirement failed");
        }
    }
}
fn factory(env: &mut JNIEnv<'_>, provider: JObject<'_>) -> Result<(JavaVM, GlobalRef, GlobalRef)> {
    if provider.is_null() {
        return Err(Error::Invalid("recogniser factory"));
    }
    let class = env
        .find_class("org/stemnotebook/backend/NativeBridge$EngineControl")
        .map_err(|_| Error::WorkerFailed)?;
    Ok((
        env.get_java_vm().map_err(|_| Error::WorkerFailed)?,
        env.new_global_ref(provider)
            .map_err(|_| Error::WorkerFailed)?,
        env.new_global_ref(class).map_err(|_| Error::WorkerFailed)?,
    ))
}
fn construct(
    vm: &JavaVM,
    provider: &GlobalRef,
    class: &GlobalRef,
    descriptor: &Descriptor,
    resource: Option<&[u8]>,
    control: &JobControl,
) -> Result<Box<dyn Recogniser>> {
    control.check()?;
    if resource.is_some_and(|r| r.len() > i32::MAX as usize) {
        return Err(Error::ResourceLimit);
    }
    let ticket = CallbackControl::new(control)?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|_| Error::WorkerFailed)?;
    frame(&mut env, 12, |env| -> Result<Box<dyn Recogniser>> {
        let adapter_vm = env.get_java_vm().map_err(|_| Error::WorkerFailed)?;
        let descriptor = env
            .new_string(serde_json::to_string(descriptor)?)
            .map_err(|_| Error::WorkerFailed)?;
        let control_object = control_object(env, class, ticket.0)?;
        let bytes = match resource {
            Some(bytes) => env
                .byte_array_from_slice(bytes)
                .map_err(|_| Error::WorkerFailed)?,
            None => JByteArray::from(JObject::null()),
        };
        let result = env.call_method(provider.as_obj(), "create", "(Ljava/lang/String;[BLorg/stemnotebook/backend/NativeBridge$EngineControl;)Lorg/stemnotebook/backend/NativeBridge$Recogniser;", &[JValue::Object(&descriptor), JValue::Object(&bytes), JValue::Object(&control_object)]);
        let result = jni_result(env, result);
        // Own a returned SDK before wiping, so a later failure still retires it.
        let adapter = result.and_then(|value| {
            let object = value.l().map_err(|_| Error::WorkerFailed)?;
            if object.is_null() {
                return Err(Error::WorkerFailed);
            }
            Ok(JavaRecogniser {
                vm: adapter_vm,
                object: match env.new_global_ref(&object) {
                    Ok(owner) => owner,
                    Err(_) => {
                        clear_exception(env);
                        let result = env.call_method(&object, "retire", "()V", &[]);
                        let _ = jni_result(env, result);
                        return Err(Error::WorkerFailed);
                    }
                },
                control_class: class.clone(),
                activation: control.clone(),
            })
        });
        let mut wiped = true;
        if let Some(resource) = resource {
            let zeros = [0i8; 65536];
            for offset in (0..resource.len()).step_by(zeros.len()) {
                let n = (resource.len() - offset).min(zeros.len());
                let result = env.set_byte_array_region(&bytes, offset as i32, &zeros[..n]);
                wiped &= jni_result(env, result).is_ok();
            }
        }
        if !wiped {
            return Err(Error::WorkerFailed);
        }
        control.check()?;
        Ok(Box::new(adapter?))
    })
}
fn insert(host: i64, control: JobControl, work: Work) -> Option<i64> {
    let id = allocate_handle(&NEXT)?;
    // Serialize insertion with host removal. A close either sees this job or
    // prevents insertion; no idle token can appear after host cleanup scanned.
    let owners = hosts().lock().ok()?;
    if !owners.contains_key(&host) {
        return None;
    }
    let mut entries = jobs().lock().ok()?;
    if entries.values().filter(|entry| entry.host == host).count() >= 8 {
        return None;
    }
    entries.insert(
        id,
        Entry {
            host,
            control,
            work: Some(work),
        },
    );
    Some(id)
}
fn take(host: i64, token: i64) -> Option<(Work, JobControl, Running)> {
    let mut entries = jobs().lock().ok()?;
    let entry = entries.get_mut(&token)?;
    if entry.host != host {
        return None;
    }
    let work = entry.work.take()?;
    Some((work, entry.control.clone(), Running(token, true)))
}
fn release(host: i64, token: i64) {
    let idle = {
        let Ok(mut entries) = jobs().lock() else {
            return;
        };
        let Some(entry) = entries.get(&token).filter(|e| e.host == host) else {
            return;
        };
        entry.control.cancel();
        if entry.work.is_some() {
            entries.remove(&token)
        } else {
            None
        }
    };
    drop(idle);
}
pub(super) fn close(host: i64) {
    let idle = {
        let Ok(mut entries) = jobs().lock() else {
            return;
        };
        let ids: Vec<_> = entries
            .iter()
            .filter(|(_, e)| e.host == host)
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| {
                let e = entries.get(&id)?;
                e.control.cancel();
                if e.work.is_some() {
                    entries.remove(&id)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    drop(idle);
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_checkEngineControl(
    _: JNIEnv<'_>,
    _: JClass<'_>,
    token: jlong,
) -> jboolean {
    controls()
        .lock()
        .ok()
        .and_then(|map| map.get(&token).map(|c| c.check().is_ok()))
        .unwrap_or(false)
        .into()
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_releaseEngineJob(
    _: JNIEnv<'_>,
    _: JClass<'_>,
    host: jlong,
    token: jlong,
) {
    release(host, token);
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_stageResourceFreeEngine(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    descriptor: JString<'_>,
    confirmed: jboolean,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let descriptor =
            serde_json::from_str(&bounded_string(&mut env, &descriptor, 65536)?).ok()?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let version = host
            .stage_resource_free_engine(descriptor, confirmed != 0)
            .ok()?;
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
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_beginEngineActivation(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    version: JString<'_>,
    max_load: jlong,
    budget: jlong,
) -> jlong {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<i64> {
        if !(1..=3600000).contains(&budget) || !(0..=i32::MAX as i64).contains(&max_load) {
            return None;
        }
        let control = JobControl::new(Duration::from_millis(budget as u64)).ok()?;
        let version = Id::parse(&bounded_string(&mut env, &version, 36)?).ok()?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        insert(
            handle,
            control,
            Work::Activation(host.engine_activation_plan(version).ok()?, max_load as u64),
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_beginEngineRecognition(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    block: JString<'_>,
    region: JString<'_>,
    task: JString<'_>,
    engine: JString<'_>,
    budget: jlong,
) -> jlong {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<i64> {
        if !(1..=3600000).contains(&budget) {
            return None;
        }
        let control = JobControl::new(Duration::from_millis(budget as u64)).ok()?;
        let block = Id::parse(&bounded_string(&mut env, &block, 36)?).ok()?;
        let region = if region.is_null() {
            None
        } else {
            Some(Id::parse(&bounded_string(&mut env, &region, 36)?).ok()?)
        };
        let task = match bounded_string(&mut env, &task, 16)?.as_str() {
            "Maths" => Task::Maths,
            "Prose" => Task::Prose,
            "Numeric" => Task::Numeric,
            "Code" => Task::Code,
            _ => return None,
        };
        let engine = if engine.is_null() {
            None
        } else {
            Some(bounded_string(&mut env, &engine, 256)?)
        };
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        insert(
            handle,
            control,
            Work::Recognition(
                host.prepare_runtime_recognition(block, region, task, engine.as_deref())
                    .ok()?,
            ),
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_runEngineActivation(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    provider: JObject<'_>,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<()> {
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let (work, control, _running) = take(handle, token)?;
        let Work::Activation(plan, max_load) = work else {
            return None;
        };
        let (vm, provider, class) = factory(&mut env, provider).ok()?;
        let prepared = plan
            .prepare_recogniser_with(max_load, &control, |d, resource, c| {
                construct(&vm, &provider, &class, d, resource, c)
            })
            .ok()?;
        host.install_prepared_engine(&prepared).ok()?;
        Some(())
    }));
    if !matches!(result, Ok(Some(()))) {
        clear_exception(&mut env);
        fail(&mut env);
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_runEngineRecognition(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let (work, control, _running) = take(handle, token)?;
        let Work::Recognition(work) = work else {
            return None;
        };
        let completed = work.run(&control).ok()?;
        let proposal = host.publish_recognition(completed).ok()?;
        Some(
            env.new_string(serde_json::to_string(&proposal).ok()?)
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

/// Metadata-only routes clone the host owner before waiting for the writer.
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_engineCatalogue(
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
        let after = if after.is_null() {
            None
        } else {
            Some(Id::parse(&bounded_string(&mut env, &after, 36)?).ok()?)
        };
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let page = host.engine_catalogue(after, limit as usize).ok()?;
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
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_selectEngineDefault(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    task: JString<'_>,
    engine: JString<'_>,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<()> {
        let task = match bounded_string(&mut env, &task, 16)?.as_str() {
            "Maths" => Task::Maths,
            "Prose" => Task::Prose,
            "Numeric" => Task::Numeric,
            "Code" => Task::Code,
            _ => return None,
        };
        let engine = bounded_string(&mut env, &engine, 256)?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        host.select_engine(task, &engine).ok()
    }));
    if !matches!(result, Ok(Some(()))) {
        clear_exception(&mut env);
        fail(&mut env);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_registry_scan_cannot_be_followed_by_late_job_admission() {
        let root = std::env::temp_dir().join(format!("stem-jni-admission-{}", Id::new()));
        std::fs::create_dir(&root).unwrap();
        let host = Arc::new(stem_core::host::Host::open(root.join("library.db"), 8).unwrap());
        let id = allocate_handle(&super::super::NEXT).unwrap();
        hosts().lock().unwrap().insert(id, Arc::clone(&host));
        let descriptor = Descriptor {
            id: "synthetic-admission".into(),
            version: "1".into(),
            adapter_version: "1".into(),
            family: stem_core::processing::Family::Recogniser,
            input: stem_core::processing::InputContract::OrderedStrokesV1,
            tasks: vec![Task::Prose],
            internals: stem_core::processing::Internals::StrokeOnlyVerified,
            offline: true,
            content_egress: false,
            personal_use_evidence: Some("app-owned registry fixture".into()),
            redistributable_resources: false,
            resource_sha256: None,
            runtime_contract: "stem-processing-v1".into(),
        };
        let version = host.stage_resource_free_engine(descriptor, true).unwrap();
        let control = JobControl::new(Duration::from_secs(30)).unwrap();
        let initial = host.engine_activation_plan(version.id).unwrap();
        insert(id, control.clone(), Work::Activation(initial, 0)).unwrap();
        let late = host.engine_activation_plan(version.id).unwrap();
        // Reproduce the close window after removal/scan but before core close:
        // an already admitted caller still has its Host and plan references.
        hosts().lock().unwrap().remove(&id);
        close(id);
        let admitted = insert(id, control, Work::Activation(late, 0));
        if let Some(token) = admitted {
            release(id, token);
        }
        assert!(admitted.is_none());
        assert!(!jobs()
            .lock()
            .unwrap()
            .values()
            .any(|entry| entry.host == id));
        host.close().unwrap();
        drop(host);
        std::fs::remove_dir_all(root).unwrap();
    }
}
