//! Platform-independent pen routing and bounded persistence admission. The Android
//! adapter supplies classified observed events; predictions never enter this API.
use crate::{
    ink::{Interruption, Point, Stroke, StrokeEnd, Tool},
    processing::JobControl,
    security::Access,
    store::{Block, Store, WritingTarget},
    Error, Id, Result,
};

use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerKind {
    Pen,
    Eraser,
    Touch,
    Palm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Routing {
    Drawing,
    Navigation,
    Ignored,
}

pub struct InputRouter {
    session: Id,
    space: String,
    next: u64,
    active: BTreeMap<u32, Stroke>,
    finger_ink: bool,
    max_points: usize,
}

impl InputRouter {
    pub fn new(space: &str, finger_ink: bool, max_points: usize) -> Result<Self> {
        if space.is_empty() || space.len() > 256 || max_points == 0 || max_points > 100000 {
            return Err(Error::Invalid("capture configuration"));
        }

        Ok(Self {
            session: Id::new(),
            space: space.into(),
            next: 0,
            active: BTreeMap::new(),
            finger_ink,
            max_points,
        })
    }

    pub fn begin(&mut self, pointer: u32, kind: PointerKind, point: Point) -> Result<Routing> {
        if self.active.contains_key(&pointer) {
            return Err(Error::Conflict);
        }

        if kind == PointerKind::Palm {
            return Ok(Routing::Ignored);
        }

        let pen_active = self.active.values().any(|s| s.tool != Tool::ExplicitFinger);

        if kind == PointerKind::Touch && pen_active {
            return Ok(Routing::Ignored);
        }

        if kind == PointerKind::Touch && !self.finger_ink {
            return Ok(Routing::Navigation);
        }

        let tool = match kind {
            PointerKind::Pen => Tool::Pen,
            PointerKind::Eraser => Tool::Eraser,
            PointerKind::Touch => Tool::ExplicitFinger,
            PointerKind::Palm => unreachable!(),
        };

        let s = Stroke {
            id: Id::new(),
            capture_session: self.session,
            sequence: self.next,
            pointer_id: pointer,
            coordinate_space: self.space.clone(),
            tool,
            end: StrokeEnd::InProgress,
            points: vec![point],
        };

        s.validate()?;
        let retained = self
            .active
            .values()
            .filter(|s| kind == PointerKind::Touch || s.tool != Tool::ExplicitFinger)
            .collect::<Vec<_>>();
        if retained.len() >= 32
            || retained.iter().map(|s| s.points.len()).sum::<usize>() >= self.max_points
        {
            return Err(Error::ResourceLimit);
        }

        // A newly detected stylus cancels tentative finger ink, not a committed pen stroke.
        if kind != PointerKind::Touch {
            self.active.retain(|_, s| s.tool != Tool::ExplicitFinger);
        }

        self.next = self.next.checked_add(1).ok_or(Error::ResourceLimit)?;

        self.active.insert(pointer, s);

        Ok(Routing::Drawing)
    }

    pub fn sample(&mut self, pointer: u32, point: Point) -> Result<()> {
        if self.active.values().map(|s| s.points.len()).sum::<usize>() >= self.max_points {
            return Err(Error::ResourceLimit);
        }
        let s = self.active.get_mut(&pointer).ok_or(Error::Unavailable)?;

        if s.points.len() >= self.max_points {
            return Err(Error::ResourceLimit);
        }

        let probe = Stroke {
            points: vec![point.clone()],
            ..s.clone_header()
        };

        probe.validate()?;

        if point.time_us < s.points.last().expect("nonempty capture").time_us {
            return Err(Error::Invalid("event order"));
        }

        s.points.push(point);

        Ok(())
    }

    pub fn finish(&mut self, pointer: u32) -> Result<Stroke> {
        let mut stroke = self.active.remove(&pointer).ok_or(Error::Unavailable)?;
        stroke.end = StrokeEnd::Completed;
        Ok(stroke)
    }
    /// Immutable exact observed prefixes. This is not a durable-save receipt.
    pub fn checkpoint(&self) -> Vec<Stroke> {
        let mut strokes = self.active.values().cloned().collect::<Vec<_>>();
        strokes.sort_by_key(|s| s.sequence);
        strokes
    }
    /// Seal only observed samples, without adding pen-up coordinates or timestamps.
    pub fn interrupt(&mut self, reason: Interruption) -> Vec<Stroke> {
        let mut strokes = std::mem::take(&mut self.active)
            .into_values()
            .collect::<Vec<_>>();
        for stroke in &mut strokes {
            stroke.end = StrokeEnd::Interrupted(reason);
        }
        strokes.sort_by_key(|s| s.sequence);
        strokes
    }

    pub fn cancel(&mut self, pointer: u32) -> bool {
        self.active.remove(&pointer).is_some()
    }

    pub fn cancel_all(&mut self) {
        self.active.clear();
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }
}

impl Stroke {
    fn clone_header(&self) -> Self {
        Self {
            id: self.id,
            capture_session: self.capture_session,
            sequence: self.sequence,
            pointer_id: self.pointer_id,
            coordinate_space: self.coordinate_space.clone(),
            tool: self.tool.clone(),
            end: self.end,
            points: vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct PendingCapture {
    pub ticket: Id,
    pub target: WritingTarget,
    pub strokes: Vec<Stroke>,
}

struct Queued {
    capture: PendingCapture,
    bytes: usize,
}

pub struct PersistenceQueue {
    pending: VecDeque<Queued>,
    bytes: usize,
    max_bytes: usize,
    max_batches: usize,
}

impl PersistenceQueue {
    pub fn new(max_bytes: usize, max_batches: usize) -> Result<Self> {
        if max_bytes == 0 || max_batches == 0 {
            return Err(Error::Invalid("queue capacity"));
        }

        Ok(Self {
            pending: VecDeque::new(),
            bytes: 0,
            max_bytes,
            max_batches,
        })
    }

    /// Caller retains the original value on rejection. No old work is evicted.
    pub fn enqueue(&mut self, capture: &PendingCapture) -> Result<()> {
        if capture.strokes.is_empty() {
            return Err(Error::Invalid("empty capture"));
        }

        for s in &capture.strokes {
            s.validate_persisted()?;
        }

        let bytes = serde_json::to_vec(&capture.strokes)?.len();

        if self
            .pending
            .iter()
            .any(|q| q.capture.ticket == capture.ticket)
        {
            return Err(Error::Conflict);
        }

        if self.pending.len() >= self.max_batches
            || bytes > self.max_bytes.saturating_sub(self.bytes)
        {
            return Err(Error::ResourceLimit);
        }

        self.pending.push_back(Queued {
            capture: capture.clone(),
            bytes,
        });

        self.bytes += bytes;

        Ok(())
    }

    /// Run on the persistence worker. Failed writes stay at the head for retry;
    /// SQLite receipts make retries safe after ambiguous acknowledgement.
    pub fn persist_next(&mut self, store: &mut Store, access: &Access) -> Result<Option<Block>> {
        let Some(q) = self.pending.front() else {
            return Ok(None);
        };

        let b = store.append_capture(
            q.capture.ticket,
            q.capture.target,
            q.capture.strokes.clone(),
            access,
        )?;

        let q = self.pending.pop_front().expect("front retained");

        self.bytes -= q.bytes;

        Ok(Some(b))
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn pending_bytes(&self) -> usize {
        self.bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkClass {
    Recognition,
    PdfRender,
    Index,
    Export,
    Thumbnail,
}

struct Running {
    bytes: usize,
    control: JobControl,
}

struct Admission {
    capture: usize,
    used: usize,
    limit: usize,
    running: BTreeMap<Id, Running>,
}

#[derive(Clone)]
pub struct Scheduler {
    state: Arc<Mutex<Admission>>,
}

pub struct WorkLease {
    state: Arc<Mutex<Admission>>,
    id: Id,
    pub control: JobControl,
}

pub struct CapturePriority {
    state: Arc<Mutex<Admission>>,
}

impl Scheduler {
    pub fn new(memory_limit: usize) -> Result<Self> {
        if memory_limit == 0 {
            return Err(Error::Invalid("memory budget"));
        }

        Ok(Self {
            state: Arc::new(Mutex::new(Admission {
                capture: 0,
                used: 0,
                limit: memory_limit,
                running: BTreeMap::new(),
            })),
        })
    }

    pub fn begin_capture(&self) -> Result<CapturePriority> {
        let mut s = self.state.lock().map_err(|_| Error::WorkerFailed)?;

        s.capture += 1;

        for r in s.running.values() {
            r.control.cancel();
        }

        Ok(CapturePriority {
            state: Arc::clone(&self.state),
        })
    }

    pub fn admit(&self, _class: WorkClass, bytes: usize, budget: Duration) -> Result<WorkLease> {
        let mut s = self.state.lock().map_err(|_| Error::WorkerFailed)?;

        if s.capture > 0 {
            return Err(Error::Cancelled);
        }

        if bytes > s.limit.saturating_sub(s.used) {
            return Err(Error::ResourceLimit);
        }

        let control = JobControl::new(budget)?;

        let id = Id::new();

        s.used += bytes;

        s.running.insert(
            id,
            Running {
                bytes,
                control: control.clone(),
            },
        );

        Ok(WorkLease {
            state: Arc::clone(&self.state),
            id,
            control,
        })
    }

    pub fn reserved_bytes(&self) -> Result<usize> {
        Ok(self.state.lock().map_err(|_| Error::WorkerFailed)?.used)
    }
}

impl Drop for WorkLease {
    fn drop(&mut self) {
        if let Ok(mut s) = self.state.lock() {
            if let Some(r) = s.running.remove(&self.id) {
                s.used -= r.bytes;
            }
        }
    }
}

impl Drop for CapturePriority {
    fn drop(&mut self) {
        if let Ok(mut s) = self.state.lock() {
            s.capture = s.capture.saturating_sub(1);
        }
    }
}

/// A deliberately written reserve, not a sparse length claim. The host observes
/// free space and releases this file before emergency ink flush. No zero-loss claim.
pub struct EmergencyReserve {
    path: PathBuf,
    pub bytes: u64,
}

impl EmergencyReserve {
    pub fn create(path: impl AsRef<Path>, bytes: u64) -> Result<Self> {
        if bytes == 0 || bytes > 64 * 1024 * 1024 {
            return Err(Error::Invalid("reserve size"));
        }

        let path = path.as_ref().to_path_buf();

        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;

        let result = (|| -> std::io::Result<()> {
            let zeros = [0u8; 65536];

            let mut remaining = bytes;

            while remaining > 0 {
                let n = remaining.min(zeros.len() as u64) as usize;

                f.write_all(&zeros[..n])?;

                remaining -= n as u64;
            }

            f.sync_all()
        })();

        if let Err(e) = result {
            drop(f);

            let _ = std::fs::remove_file(&path);

            return Err(e.into());
        }

        Ok(Self { path, bytes })
    }

    pub fn release(self) -> Result<u64> {
        std::fs::remove_file(&self.path)?;

        Ok(self.bytes)
    }

    pub fn low_space(available: u64, reserve: u64, next_write: u64) -> bool {
        available < reserve.saturating_add(next_write)
    }
}
