//! Metadata capture, optional-worker verification/probe, then callback-free
//! durable publication. The existing Manager is absent from probe preparation.
use super::*;
use crate::processing::{Converter, DetachedRuntime, JobControl, ReadyRuntime};
use std::time::Instant;

pub struct EngineActivationPlan {
    library: String,
    generation: String,
    version: EngineVersion,
    expires: Option<Instant>,
}
pub struct PreparedEngineActivation {
    plan: Option<EngineActivationPlan>,
    ready: ReadyRuntime,
    control: JobControl,
}
impl EngineActivationPlan {
    pub fn version(&self) -> &EngineVersion {
        &self.version
    }
    fn check(&self, control: &JobControl) -> Result<()> {
        if self.expires.is_some_and(|until| Instant::now() >= until) {
            return Err(Error::Denied);
        }
        control.check()
    }
    fn resource(
        &self,
        max_load_bytes: u64,
        control: &JobControl,
    ) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>> {
        self.check(control)?;
        if let Some(path) = &self.version.local_resource {
            let file = std::fs::File::open(path)?;
            if file.metadata()?.len() > max_load_bytes {
                return Err(Error::ResourceLimit);
            }
            let mut bytes = zeroize::Zeroizing::new(Vec::new());
            let mut input = file.take(max_load_bytes.saturating_add(1));
            let mut chunk = zeroize::Zeroizing::new([0u8; 65536]);
            loop {
                self.check(control)?;
                let n = input.read(&mut chunk[..])?;
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..n]);
                if bytes.len() as u64 > max_load_bytes {
                    return Err(Error::ResourceLimit);
                }
            }
            if Some(digest(&bytes)) != self.version.descriptor.resource_sha256 {
                return Err(Error::Integrity);
            }
            Ok(Some(bytes))
        } else if self.version.descriptor.resource_sha256.is_some() {
            Err(Error::Integrity)
        } else {
            Ok(None)
        }
    }
    fn finish(
        self,
        mut candidate: Manager,
        resource: Option<zeroize::Zeroizing<Vec<u8>>>,
        control: &JobControl,
    ) -> Result<PreparedEngineActivation> {
        let id = &self.version.descriptor.id;
        let work =
            candidate.prepare_activation(id, resource.as_ref().map(|r| r.as_slice()), true)?;
        let mut completed = work.run(control);
        candidate.complete_activation(&mut completed)?;
        self.check(control)?;
        let ready = candidate.take_ready(id)?;
        Ok(PreparedEngineActivation {
            plan: Some(self),
            ready,
            control: control.clone(),
        })
    }
    /// Call on an optional worker after releasing the live Manager lock.
    pub fn prepare_recogniser(
        self,
        adapter: Box<dyn Recogniser>,
        max_load_bytes: u64,
        control: &JobControl,
    ) -> Result<PreparedEngineActivation> {
        self.prepare_recogniser_with(max_load_bytes, control, |_, _, _| Ok(adapter))
    }
    /// Construct the SDK from the exact bounded, hash-verified resource snapshot.
    /// The bytes are temporary and zero-cleared after preparation; an SDK must
    /// finish loading or retain its own protected copy before returning.
    pub fn prepare_recogniser_with(
        self,
        max_load_bytes: u64,
        control: &JobControl,
        factory: impl FnOnce(&Descriptor, Option<&[u8]>, &JobControl) -> Result<Box<dyn Recogniser>>,
    ) -> Result<PreparedEngineActivation> {
        Manager::validate_recogniser(&self.version.descriptor)?;
        let resource = self.resource(max_load_bytes, control)?;
        self.check(control)?;
        let adapter = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            factory(
                &self.version.descriptor,
                resource.as_ref().map(|r| r.as_slice()),
                control,
            )
        }))
        .map_err(|_| Error::WorkerFailed)??;
        self.check(control)?;
        let mut candidate = Manager::new();
        candidate.register_recogniser(self.version.descriptor.clone(), adapter)?;
        self.finish(candidate, resource, control)
    }
    pub fn prepare_converter(
        self,
        adapter: Box<dyn Converter>,
        max_load_bytes: u64,
        control: &JobControl,
    ) -> Result<PreparedEngineActivation> {
        self.prepare_converter_with(max_load_bytes, control, |_, _, _| Ok(adapter))
    }
    pub fn prepare_converter_with(
        self,
        max_load_bytes: u64,
        control: &JobControl,
        factory: impl FnOnce(&Descriptor, Option<&[u8]>, &JobControl) -> Result<Box<dyn Converter>>,
    ) -> Result<PreparedEngineActivation> {
        Manager::validate_converter(&self.version.descriptor)?;
        let resource = self.resource(max_load_bytes, control)?;
        self.check(control)?;
        let adapter = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            factory(
                &self.version.descriptor,
                resource.as_ref().map(|r| r.as_slice()),
                control,
            )
        }))
        .map_err(|_| Error::WorkerFailed)??;
        self.check(control)?;
        let mut candidate = Manager::new();
        candidate.register_converter(self.version.descriptor.clone(), adapter)?;
        self.finish(candidate, resource, control)
    }
}
impl PreparedEngineActivation {
    pub fn version(&self) -> Option<&EngineVersion> {
        self.plan.as_ref().map(|plan| &plan.version)
    }
}
impl Store {
    /// Capture metadata only; local resource I/O and provider callbacks are later
    /// optional-worker work. Ordinary notebook data cannot construct this plan.
    pub fn engine_activation_plan(&self, id: Id, access: &Access) -> Result<EngineActivationPlan> {
        let version = self.engine_version(id, access)?;
        if version.state == Installation::Removed {
            return Err(Error::Unavailable);
        }
        Ok(EngineActivationPlan {
            library: Self::setting(&self.db, "library_id")?,
            generation: Self::setting(&self.db, "generation")?,
            version,
            expires: access.expires_at(),
        })
    }
    /// No provider callback or disposal: failure leaves the candidate holder
    /// intact; success returns the old owner for disposal after unlocking.
    pub fn commit_engine_activation(
        &mut self,
        prepared: &mut PreparedEngineActivation,
        manager: &mut Manager,
        access: &Access,
    ) -> Result<Option<DetachedRuntime>> {
        Self::require_app(&self.db, access)?;
        let plan = prepared.plan.as_ref().ok_or(Error::Conflict)?;
        plan.check(&prepared.control)?;
        if Self::setting(&self.db, "library_id")? != plan.library
            || Self::setting(&self.db, "generation")? != plan.generation
            || self.engine_version(plan.version.id, access)? != plan.version
        {
            return Err(Error::Conflict);
        }
        if prepared.ready.descriptor() != Some(&plan.version.descriptor) {
            return Err(Error::Integrity);
        }
        // Exclusive Manager/candidate borrows make publication infallible after
        // this preflight, while failure does not dispose either runtime.
        let defaults = self
            .engine_defaults(access)?
            .into_iter()
            .filter_map(|(task, engine)| {
                (engine == plan.version.descriptor.id
                    && plan.version.descriptor.family == crate::processing::Family::Recogniser
                    && plan.version.descriptor.tasks.contains(&task))
                .then_some(task)
            })
            .collect();
        let publication = manager
            .ready_publication(&mut prepared.ready)?
            .with_defaults(defaults)?;
        let mut version = plan.version.clone();
        version.state = Installation::Installed;
        version.installed_ms = Some(now_ms());
        self.write(|db| {
            Self::require_app(db,access)?;plan.check(&prepared.control)?;
            if Self::setting(db,"generation")?!=plan.generation {return Err(Error::Conflict);}
            db.execute("UPDATE engine_versions SET payload=?1 WHERE id=?2",rusqlite::params![serde_json::to_string(&version)?,version.id.to_string()])?;
            db.execute("INSERT INTO engine_active VALUES(?1,?2) ON CONFLICT(engine) DO UPDATE SET version=excluded.version",rusqlite::params![version.descriptor.id,version.id.to_string()])?;
            plan.check(&prepared.control)?;Ok(())
        })?;
        let retired = publication.commit();
        prepared.plan = None;
        Ok(retired)
    }
}
