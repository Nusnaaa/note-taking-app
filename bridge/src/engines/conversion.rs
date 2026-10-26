//! Typed content-converter callbacks and move-only native conversion work.
use super::*;
use stem_core::{
    content::Content,
    processing::{Converted, Converter, Format},
};

struct JavaConverter {
    owner: JavaRecogniser,
    formats: OnceLock<Vec<Format>>,
}
impl Converter for JavaConverter {
    fn probe(&self) -> Result<()> {
        self.owner.probe()?;
        let control = &self.owner.activation;
        let ticket = CallbackControl::new(control)?;
        let mut env = self
            .owner
            .vm
            .attach_current_thread()
            .map_err(|_| Error::WorkerFailed)?;
        let formats = frame(&mut env, 8, |env| {
            let object = control_object(env, &self.owner.control_class, ticket.0)?;
            let result = env.call_method(
                self.owner.object.as_obj(),
                "formats",
                "(Lorg/stemnotebook/backend/NativeBridge$EngineControl;)Ljava/lang/String;",
                &[JValue::Object(&object)],
            );
            let value = jni_result(env, result)?
                .l()
                .map_err(|_| Error::WorkerFailed)?;
            if value.is_null() {
                return Err(Error::WorkerFailed);
            }
            let json =
                bounded_string(env, &JString::from(value), 1024).ok_or(Error::ResourceLimit)?;
            let formats: Vec<Format> = serde_json::from_str(&json)?;
            if formats.is_empty()
                || formats.len() > 5
                || formats
                    .iter()
                    .enumerate()
                    .any(|(i, f)| formats[..i].contains(f))
            {
                return Err(Error::Invalid("converter formats"));
            }
            control.check()?;
            Ok(formats)
        })?;
        self.formats.set(formats).map_err(|_| Error::Conflict)
    }
    fn formats(&self) -> Vec<Format> {
        self.formats.get().cloned().unwrap_or_default()
    }
    fn convert(
        &self,
        content: &Content,
        target: Format,
        control: &JobControl,
    ) -> Result<Converted> {
        control.check()?;
        let content = serde_json::to_string(content)?;
        if content.len() > MAX_JSON {
            return Err(Error::ResourceLimit);
        }
        let target = format_name(&target);
        let ticket = CallbackControl::new(control)?;
        let mut env = self
            .owner
            .vm
            .attach_current_thread()
            .map_err(|_| Error::WorkerFailed)?;
        frame(&mut env, 12, |env| {
            let object = control_object(env, &self.owner.control_class, ticket.0)?;
            let content = env.new_string(content).map_err(|_| Error::WorkerFailed)?;
            let target = env.new_string(target).map_err(|_| Error::WorkerFailed)?;
            let result = env.call_method(self.owner.object.as_obj(), "convert",
                "(Ljava/lang/String;Ljava/lang/String;Lorg/stemnotebook/backend/NativeBridge$EngineControl;)Ljava/lang/String;",
                &[JValue::Object(&content), JValue::Object(&target), JValue::Object(&object)]);
            let value = jni_result(env, result)?
                .l()
                .map_err(|_| Error::WorkerFailed)?;
            if value.is_null() {
                return Err(Error::WorkerFailed);
            }
            let json = bounded_string(env, &JString::from(value), MAX_JSON as i32)
                .ok_or(Error::ResourceLimit)?;
            if json.len() > MAX_JSON {
                return Err(Error::ResourceLimit);
            }
            control.check()?;
            Ok(serde_json::from_str(&json)?)
        })
    }
}
fn format_name(format: &Format) -> &'static str {
    match format {
        Format::Markdown => "Markdown",
        Format::Latex => "Latex",
        Format::MathMl => "MathMl",
        Format::PlainText => "PlainText",
        Format::Native => "Native",
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_runConverterActivation(
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
        let prepared = plan.prepare_converter_with(max_load, &control, |d, resource, c| {
            Ok(Box::new(JavaConverter { owner: construct_owner(&vm, &provider, &class, d, resource, c,
                "(Ljava/lang/String;[BLorg/stemnotebook/backend/NativeBridge$EngineControl;)Lorg/stemnotebook/backend/NativeBridge$Converter;")?, formats: OnceLock::new() }))
        }).ok()?;
        host.install_prepared_engine(&prepared).ok()?;
        Some(())
    }));
    if !matches!(result, Ok(Some(()))) {
        clear_exception(&mut env);
        fail(&mut env);
    }
}
#[no_mangle]
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_beginEngineConversion(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    block: JString<'_>,
    region: JString<'_>,
    engine: JString<'_>,
    target: JString<'_>,
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
        let engine = bounded_string(&mut env, &engine, 256)?;
        let target = match bounded_string(&mut env, &target, 16)?.as_str() {
            "Markdown" => Format::Markdown,
            "Latex" => Format::Latex,
            "MathMl" => Format::MathMl,
            "PlainText" => Format::PlainText,
            "Native" => Format::Native,
            _ => return None,
        };
        let host = Arc::clone(hosts().lock().ok()?.get(&handle)?);
        insert(
            handle,
            control,
            Work::Conversion(
                host.prepare_runtime_conversion(block, region, &engine, target)
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
pub extern "system" fn Java_org_stemnotebook_backend_NativeBridge_runEngineConversion(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    token: jlong,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<jstring> {
        let (work, control, _running) = take(handle, token)?;
        let Work::Conversion(work) = work else {
            return None;
        };
        let completed = work.run(&control).ok()?;
        let (block, region, revision) = completed.source();
        let json = serde_json::to_string(&serde_json::json!({ "block":block, "region":region, "revision":revision,
            "descriptor":completed.descriptor(), "target":completed.target(), "output":completed.output().ok()? })).ok()?;
        if json.len() > MAX_JSON {
            return None;
        }
        completed.output().ok()?; // authority/expiry/runtime check after encoding
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
