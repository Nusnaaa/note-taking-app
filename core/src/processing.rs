//! Typed local processing contracts. Executable adapters are compiled into the app.
//! This manager does not download code, open sockets, or own document storage.
use crate::{content::Content, digest, store::InkSnapshot, Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Task {
    Maths,
    Prose,
    Numeric,
    Code,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputContract {
    OrderedStrokesV1,
    MandatoryRaster,
    EditableV1,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Internals {
    StrokeOnlyVerified,
    Undisclosed,
    RasterRequired,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Family {
    Recogniser,
    Converter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub id: String,
    pub version: String,
    pub adapter_version: String,
    pub family: Family,
    pub input: InputContract,
    pub tasks: Vec<Task>,
    pub internals: Internals,
    pub offline: bool,
    pub content_egress: bool,
    /// Citation/receipt of a real personal-use licence review, not a commercial gate.
    pub personal_use_evidence: Option<String>,
    pub redistributable_resources: bool,
    pub resource_sha256: Option<String>,
    pub runtime_contract: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    Missing,
    Ready,
    Failed,
}

#[derive(Clone)]
pub struct JobControl {
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}
impl JobControl {
    pub fn new(budget: Duration) -> Result<Self> {
        let deadline = Instant::now()
            .checked_add(budget)
            .ok_or(Error::Invalid("job duration"))?;
        Ok(Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline,
        })
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(Error::Timeout);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecognitionOutput {
    pub candidates: Vec<Content>,
    pub raw_output: String,
    /// None means unavailable/uncalibrated; no fabricated confidence scores.
    pub confidence: Option<f64>,
    pub diagnostics: Vec<String>,
}

pub trait Recogniser: Send + Sync {
    fn probe(&self) -> Result<()>;
    fn recognise(
        &self,
        ink: &InkSnapshot,
        task: Task,
        control: &JobControl,
    ) -> Result<RecognitionOutput>;
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Format {
    Markdown,
    Latex,
    MathMl,
    PlainText,
    Native,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fidelity {
    pub complete: bool,
    pub lexical_change: bool,
    pub layout_change: bool,
    pub unsupported: Vec<String>,
    pub semantic_equivalence_asserted: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Converted {
    pub text: String,
    pub fidelity: Fidelity,
}
pub trait Converter: Send + Sync {
    fn probe(&self) -> Result<()> {
        Ok(())
    }
    fn formats(&self) -> Vec<Format>;
    fn convert(&self, content: &Content, target: Format, control: &JobControl)
        -> Result<Converted>;
}

enum Adapter {
    Recogniser(Box<dyn Recogniser>),
    Converter(Box<dyn Converter>),
}
struct Entry {
    descriptor: Descriptor,
    adapter: Adapter,
    readiness: Readiness,
}
#[derive(Default)]
pub struct Manager {
    entries: BTreeMap<String, Entry>,
    defaults: BTreeMap<Task, String>,
}

/// Source binding and engine provenance accompany output into the core proposal.
#[derive(Debug, Clone)]
pub struct CompletedRecognition {
    pub(crate) snapshot: InkSnapshot,
    pub(crate) descriptor: Descriptor,
    pub(crate) output: RecognitionOutput,
}
impl Manager {
    pub fn new() -> Self {
        Self::default()
    }
    pub(crate) fn validate(d: &Descriptor) -> Result<()> {
        if d.id.is_empty()
            || d.id.len() > 256
            || d.version.is_empty()
            || d.adapter_version.is_empty()
            || d.runtime_contract != "stem-processing-v1"
        {
            return Err(Error::EngineUnavailable);
        }
        if !d.offline
            || d.content_egress
            || d.personal_use_evidence
                .as_ref()
                .is_none_or(|s| s.is_empty())
        {
            return Err(Error::Denied);
        }
        if matches!(d.input, InputContract::MandatoryRaster)
            || d.internals == Internals::RasterRequired
        {
            return Err(Error::Unsupported);
        }
        Ok(())
    }
    pub fn register_recogniser(
        &mut self,
        descriptor: Descriptor,
        adapter: Box<dyn Recogniser>,
    ) -> Result<()> {
        Self::validate(&descriptor)?;
        if descriptor.family != Family::Recogniser
            || descriptor.input != InputContract::OrderedStrokesV1
            || descriptor.tasks.is_empty()
            || descriptor.internals != Internals::StrokeOnlyVerified
        {
            return Err(Error::EngineUnavailable);
        }
        if self.entries.contains_key(&descriptor.id) {
            return Err(Error::Conflict);
        }
        self.entries.insert(
            descriptor.id.clone(),
            Entry {
                descriptor,
                adapter: Adapter::Recogniser(adapter),
                readiness: Readiness::Missing,
            },
        );
        Ok(())
    }
    pub fn register_converter(
        &mut self,
        descriptor: Descriptor,
        adapter: Box<dyn Converter>,
    ) -> Result<()> {
        Self::validate(&descriptor)?;
        if descriptor.family != Family::Converter || descriptor.input != InputContract::EditableV1 {
            return Err(Error::EngineUnavailable);
        }
        if self.entries.contains_key(&descriptor.id) {
            return Err(Error::Conflict);
        }
        self.entries.insert(
            descriptor.id.clone(),
            Entry {
                descriptor,
                adapter: Adapter::Converter(adapter),
                readiness: Readiness::Missing,
            },
        );
        Ok(())
    }
    /// Validates an already provisioned local resource; never interprets it as code.
    /// Resource-free built-in test/converter adapters use None. Runtime-owned models
    /// need their own adapter probe; readiness cannot be inferred from a filename.
    pub fn activate_local(
        &mut self,
        id: &str,
        resource: Option<&[u8]>,
        consented: bool,
    ) -> Result<()> {
        if !consented {
            return Err(Error::Denied);
        }
        let entry = self.entries.get_mut(id).ok_or(Error::EngineUnavailable)?;
        // Re-verification starts fail-closed. A failed resource check cannot leave
        // stale Ready state from a previous activation. Updates will later stage
        // separate old/new entries; this operation revalidates this exact entry.
        entry.readiness = Readiness::Failed;
        if let Some(expected) = &entry.descriptor.resource_sha256 {
            if resource.is_none_or(|r| digest(r) != *expected) {
                return Err(Error::Integrity);
            }
        } else if resource.is_some() {
            return Err(Error::Invalid("unexpected model data"));
        }
        let result = match &entry.adapter {
            Adapter::Recogniser(r) => {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.probe()))
                    .map_err(|_| Error::WorkerFailed)
                    .and_then(|result| result)
            }
            Adapter::Converter(c) => {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.probe()))
                    .map_err(|_| Error::WorkerFailed)
                    .and_then(|r| r)
            }
        };
        entry.readiness = if result.is_ok() {
            Readiness::Ready
        } else {
            Readiness::Failed
        };
        result
    }
    pub fn readiness(&self, id: &str) -> Readiness {
        self.entries
            .get(id)
            .map(|e| e.readiness.clone())
            .unwrap_or(Readiness::Missing)
    }
    pub fn descriptors(&self) -> Vec<Descriptor> {
        self.entries
            .values()
            .map(|e| e.descriptor.clone())
            .collect()
    }
    pub fn select(&mut self, task: Task, id: &str) -> Result<()> {
        self.recogniser_for(task, id)?;
        self.defaults.insert(task, id.into());
        Ok(())
    }
    fn recogniser_for(&self, task: Task, id: &str) -> Result<&Entry> {
        let e = self.entries.get(id).ok_or(Error::EngineUnavailable)?;
        if e.readiness != Readiness::Ready
            || e.descriptor.family != Family::Recogniser
            || !e.descriptor.tasks.contains(&task)
        {
            return Err(Error::EngineUnavailable);
        }
        Ok(e)
    }
    pub fn remove(&mut self, id: &str) -> Result<()> {
        self.entries.remove(id).ok_or(Error::EngineUnavailable)?;
        self.defaults.retain(|_, v| v != id);
        Ok(())
    }
    /// Call on a worker, never the capture/UI thread. Late results are discarded,
    /// but a non-cooperative native runtime needs process isolation to stop its CPU.
    pub fn recognise(
        &self,
        snapshot: InkSnapshot,
        task: Task,
        override_id: Option<&str>,
        control: &JobControl,
    ) -> Result<CompletedRecognition> {
        control.check()?;
        let id = override_id
            .or_else(|| self.defaults.get(&task).map(String::as_str))
            .ok_or(Error::EngineUnavailable)?;
        let e = self.recogniser_for(task, id)?;
        let Adapter::Recogniser(adapter) = &e.adapter else {
            return Err(Error::EngineUnavailable);
        };
        let output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            adapter.recognise(&snapshot, task, control)
        }))
        .map_err(|_| Error::WorkerFailed)??;
        control.check()?;
        if output.candidates.is_empty()
            || output.candidates.len() > 32
            || output.raw_output.len() > 4_000_000
            || output.diagnostics.len() > 128
        {
            return Err(Error::ResourceLimit);
        }
        if output.confidence.is_some_and(|c| !c.is_finite()) {
            return Err(Error::Invalid("confidence"));
        }
        for c in &output.candidates {
            c.validate()?;
            let matching = matches!(
                (task, c),
                (Task::Maths, Content::Maths(_))
                    | (Task::Prose, Content::Text(_))
                    | (Task::Numeric, Content::Numeric(_))
                    | (Task::Code, Content::Code { .. })
            );
            if !matching {
                return Err(Error::Invalid("recognition output modality"));
            }
        }
        Ok(CompletedRecognition {
            snapshot,
            descriptor: e.descriptor.clone(),
            output,
        })
    }
    pub fn convert(
        &self,
        id: &str,
        content: &Content,
        target: Format,
        control: &JobControl,
    ) -> Result<Converted> {
        control.check()?;
        content.validate()?;
        let e = self.entries.get(id).ok_or(Error::EngineUnavailable)?;
        if e.readiness != Readiness::Ready {
            return Err(Error::EngineUnavailable);
        }
        let Adapter::Converter(c) = &e.adapter else {
            return Err(Error::EngineUnavailable);
        };
        if !c.formats().contains(&target) {
            return Err(Error::Unsupported);
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            c.convert(content, target, control)
        }))
        .map_err(|_| Error::WorkerFailed)??;
        control.check()?;
        Ok(result)
    }
}

/// A prepared replacement has passed compatibility, local integrity and a real
/// adapter probe. It cannot be constructed by notebook data or a model output.
pub(crate) struct PreparedRecogniser {
    descriptor: Descriptor,
    adapter: Box<dyn Recogniser>,
}
impl Manager {
    pub(crate) fn prepare_replacement(
        descriptor: Descriptor,
        adapter: Box<dyn Recogniser>,
        resource: Option<&[u8]>,
    ) -> Result<PreparedRecogniser> {
        let mut candidate = Self::new();
        let id = descriptor.id.clone();
        candidate.register_recogniser(descriptor, adapter)?;
        candidate.activate_local(&id, resource, true)?;
        let entry = candidate
            .entries
            .remove(&id)
            .ok_or(Error::EngineUnavailable)?;
        let Adapter::Recogniser(adapter) = entry.adapter else {
            return Err(Error::EngineUnavailable);
        };
        Ok(PreparedRecogniser {
            descriptor: entry.descriptor,
            adapter,
        })
    }
    pub(crate) fn commit_replacement(&mut self, ready: PreparedRecogniser) {
        let id = ready.descriptor.id.clone();
        self.defaults
            .retain(|task, value| value != &id || ready.descriptor.tasks.contains(task));
        self.entries.insert(
            id,
            Entry {
                descriptor: ready.descriptor,
                adapter: Adapter::Recogniser(ready.adapter),
                readiness: Readiness::Ready,
            },
        );
    }
}

pub(crate) struct PreparedConverter {
    descriptor: Descriptor,
    adapter: Box<dyn Converter>,
}
impl Manager {
    pub(crate) fn prepare_converter(
        descriptor: Descriptor,
        adapter: Box<dyn Converter>,
        resource: Option<&[u8]>,
    ) -> Result<PreparedConverter> {
        let mut candidate = Self::new();
        let id = descriptor.id.clone();
        candidate.register_converter(descriptor, adapter)?;
        candidate.activate_local(&id, resource, true)?;
        let entry = candidate
            .entries
            .remove(&id)
            .ok_or(Error::EngineUnavailable)?;
        let Adapter::Converter(adapter) = entry.adapter else {
            return Err(Error::EngineUnavailable);
        };
        Ok(PreparedConverter {
            descriptor: entry.descriptor,
            adapter,
        })
    }
    pub(crate) fn commit_converter(&mut self, ready: PreparedConverter) {
        let id = ready.descriptor.id.clone();
        self.defaults.retain(|_, value| value != &id);
        self.entries.insert(
            id,
            Entry {
                descriptor: ready.descriptor,
                adapter: Adapter::Converter(ready.adapter),
                readiness: Readiness::Ready,
            },
        );
    }
}
