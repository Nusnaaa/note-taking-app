//! Opaque two-stage derivative render/review transport; no registry lock spans
//! rendering, live writer validation or a platform verification callback.
use super::{hosts, JavaAuth};
use jni::{
    objects::{JByteArray, JClass, JObject, JString, JValue},
    sys::{jboolean, jlong, jobject, jstring},
    JNIEnv,
};
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicI64, Arc, Mutex, OnceLock},
    time::Duration,
};
use stem_core::{
    export::{Page, Selection},
    host::{DerivativeFormat, DerivativeOutput, DerivativeReview, HostDerivativeSource},
    processing::JobControl,
};
enum Phase {
    Source(Option<HostDerivativeSource>),
    Review(Option<DerivativeReview>),
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
        "Derivative export operation failed",
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_reviewDerivativeExport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    selection: JString<'_>,
    ink: jboolean,
) -> jobject {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jobject> {
        let selection: Selection =
            serde_json::from_str(&super::bounded_string(&mut env, &selection, 1024 * 1024)?)
                .ok()?;
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let source = host.derivative_source(selection, ink != 0).ok()?;
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
        match env.new_object(
            "org/stemnotebook/backend/NativeBridge$DerivativeExport",
            "(JJ)V",
            &[JValue::Long(handle), JValue::Long(token)],
        ) {
            Ok(value) => Some(value.into_raw()),
            Err(_) => {
                jobs().lock().ok()?.remove(&token);
                None
            }
        }
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_renderDerivativeExport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    font: JByteArray<'_>,
    page: JString<'_>,
    pdf: jboolean,
    budget: jlong,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        if !(1..=3_600_000).contains(&budget) {
            return None;
        }
        let (font, page) = if pdf != 0 {
            if font.is_null()
                || page.is_null()
                || env.get_array_length(&font).ok()? > 32 * 1024 * 1024
            {
                return None;
            }
            (
                Some(env.convert_byte_array(&font).ok()?),
                Some(
                    serde_json::from_str::<Page>(&super::bounded_string(&mut env, &page, 4096)?)
                        .ok()?,
                ),
            )
        } else {
            if !font.is_null() || !page.is_null() {
                return None;
            }
            (None, None)
        };
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
        let format = match (&font, page) {
            (Some(font), Some(page)) => DerivativeFormat::Pdf { font, page },
            _ => DerivativeFormat::Markdown,
        };
        let review = host.render_derivative(source, format, &control).ok()?;
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
            e.phase = Phase::Review(Some(review));
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
fn output(env: &mut JNIEnv<'_>, output: DerivativeOutput) -> Option<jobject> {
    let (kind, markdown, assets, pdf, warnings) = match output {
        DerivativeOutput::Markdown(bundle) => (
            "Markdown",
            Some(bundle.markdown.into_bytes()),
            bundle.assets,
            None,
            bundle.warnings,
        ),
        DerivativeOutput::Pdf(pdf) => ("Pdf", None, BTreeMap::new(), Some(pdf.bytes), pdf.warnings),
    };
    let kind = env.new_string(kind).ok()?;
    let markdown = match markdown {
        Some(bytes) => JObject::from(env.byte_array_from_slice(&bytes).ok()?),
        None => JObject::null(),
    };
    let pdf = match pdf {
        Some(bytes) => JObject::from(env.byte_array_from_slice(&bytes).ok()?),
        None => JObject::null(),
    };
    let names = env
        .new_object_array(
            i32::try_from(assets.len()).ok()?,
            "java/lang/String",
            JObject::null(),
        )
        .ok()?;
    let arrays = env
        .new_object_array(i32::try_from(assets.len()).ok()?, "[B", JObject::null())
        .ok()?;
    for (index, (name, bytes)) in assets.into_iter().enumerate() {
        let name = env.new_string(name).ok()?;
        let bytes = env.byte_array_from_slice(&bytes).ok()?;
        env.set_object_array_element(&names, index as i32, &name)
            .ok()?;
        env.set_object_array_element(&arrays, index as i32, &bytes)
            .ok()?;
        env.delete_local_ref(name).ok()?;
        env.delete_local_ref(bytes).ok()?;
    }
    let diagnostics = env
        .new_object_array(
            i32::try_from(warnings.len()).ok()?,
            "java/lang/String",
            JObject::null(),
        )
        .ok()?;
    for (index, warning) in warnings.into_iter().enumerate() {
        let value = env.new_string(warning).ok()?;
        env.set_object_array_element(&diagnostics, index as i32, &value)
            .ok()?;
        env.delete_local_ref(value).ok()?;
    }
    Some(
        env.new_object(
            "org/stemnotebook/backend/NativeBridge$DerivativeExportData",
            "(Ljava/lang/String;[B[Ljava/lang/String;[[B[B[Ljava/lang/String;)V",
            &[
                JValue::Object(&kind),
                JValue::Object(&markdown),
                JValue::Object(&names),
                JValue::Object(&arrays),
                JValue::Object(&pdf),
                JValue::Object(&diagnostics),
            ],
        )
        .ok()?
        .into_raw(),
    )
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_finishDerivativeExport(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
    proof: JObject<'_>,
    confirmed: jboolean,
) -> jobject {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jobject> {
        if proof.is_null() {
            return None;
        }
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        let mut auth = JavaAuth {
            vm: env.get_java_vm().ok()?,
            proof: env.new_global_ref(proof).ok()?,
        };
        let (review, control) = {
            let mut entries = jobs().lock().ok()?;
            let e = entries.get_mut(&token)?;
            if e.host != handle || e.running {
                return None;
            }
            let Phase::Review(review) = &mut e.phase else {
                return None;
            };
            let review = review.take()?;
            let control = e.control.clone()?;
            e.running = true;
            (review, control)
        };
        let _running = Running { token, keep: false };
        control.check().ok()?;
        let artifact = host
            .finish_derivative(review, &mut auth, confirmed != 0)
            .ok()?;
        control.check().ok()?;
        let result = output(&mut env, artifact)?;
        control.check().ok()?;
        Some(result)
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_releaseDerivativeExport(
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
