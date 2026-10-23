//! Versioned JSON boundary used by the native host and developer harness.
//! The bounded mailbox owns the single writer. Callers retain rejected requests;
//! only a successful response acknowledges a committed edit. Never call on UI thread.
use crate::{
    content::Content,
    ink::{Stroke, Transform},
    objects::{CellValue, DiagramPart, Rect},
    security::{Access, Authenticator, Scope},
    store::{Camera, Display, Kind, Order, Store, ViewSettings, WritingMode, WritingTarget},
    Error, Id, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    thread,
};
mod mailbox;
mod ownership;
mod pdf;
use crate::sources::PdfSelectionTarget;
use mailbox::Mailbox;
pub use mailbox::{HostStatus, StatusSubscription, WriterLimits};
use ownership::LibraryLease;
use pdf::PdfState;
pub use pdf::{PdfCommand, PdfGrantVerifier};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum Command {
    Pdf {
        command: PdfCommand,
    },
    Block {
        block: Id,
    },
    SetMode {
        entity: Id,
        mode: WritingMode,
    },
    Mode {
        entity: Id,
    },
    AppendTo {
        target: WritingTarget,
        content: Content,
    },
    CaptureTo {
        ticket: Id,
        target: WritingTarget,
        strokes: Vec<Stroke>,
    },
    CreateExercise {
        master: Id,
        title: String,
        blocks: Vec<Id>,
    },
    RenameExercise {
        exercise: Id,
        revision: u64,
        title: String,
    },
    ExerciseVisibility {
        exercise: Id,
        revision: u64,
        visible: bool,
    },
    ExerciseLock {
        exercise: Id,
        revision: u64,
        locked: bool,
    },
    SetHeading {
        block: Id,
        revision: u64,
        confirmed: bool,
    },
    OpenInMaster {
        exercise: Id,
        block: Option<Id>,
    },
    SetVisible {
        block: Id,
        revision: u64,
        visible: bool,
    },
    SetBlockLock {
        block: Id,
        revision: u64,
        locked: bool,
    },
    SetDisplay {
        block: Id,
        revision: u64,
        display: Display,
    },
    TransformSelection {
        selected: Vec<(Id, u64)>,
        transform: Transform,
    },
    EraseStrokes {
        block: Id,
        revision: u64,
        strokes: Vec<Id>,
    },
    RestoreRevision {
        block: Id,
        revision: u64,
        old_revision: u64,
    },
    Layout {
        block: Id,
    },
    SetLayout {
        block: Id,
        expected: Option<u64>,
        bounds: Rect,
        z: i32,
    },
    EditCell {
        block: Id,
        revision: u64,
        cell: Id,
        value: CellValue,
    },
    EditDiagramPart {
        block: Id,
        revision: u64,
        part: DiagramPart,
    },
    CreateGroup {
        entity: Id,
        members: Vec<Id>,
    },
    Group {
        group: Id,
    },
    TransformGroup {
        group: Id,
        revision: u64,
        transform: Transform,
    },
    Ungroup {
        group: Id,
        revision: u64,
    },
    Embed {
        embed: Id,
    },
    CreateEmbed {
        host: Id,
        source: Id,
        frame: Rect,
    },
    EditEmbed {
        embed: Id,
        revision: u64,
        camera: Camera,
        frame: Rect,
        pinned: bool,
    },
    DuplicateEmbed {
        embed: Id,
    },
    RemoveEmbed {
        embed: Id,
    },
    View {
        view: Id,
    },
    SaveView {
        view: ViewSettings,
        expected: Option<u64>,
    },
    AddLink {
        origin: Id,
        target: Id,
        anchor: Option<Id>,
    },
    Links {
        entity: Id,
        incoming: bool,
    },
    RemoveLink {
        link: Id,
    },
    List {
        parent: Option<Id>,
    },
    Create {
        kind: Kind,
        parent: Option<Id>,
        title: String,
    },
    Read {
        id: Id,
    },
    Blocks {
        entity: Id,
        offset: u32,
        limit: u32,
    },
    Append {
        entity: Id,
        content: Content,
    },
    Capture {
        ticket: Id,
        entity: Id,
        strokes: Vec<Stroke>,
    },
    Edit {
        block: Id,
        revision: u64,
        content: Content,
    },
    Rename {
        entity: Id,
        revision: u64,
        title: String,
    },
    Move {
        entity: Id,
        revision: u64,
        parent: Option<Id>,
    },
    Undo {
        entity: Id,
    },
    Redo {
        entity: Id,
    },
    Search {
        query: String,
    },
    Exercises {
        master: Id,
    },
    ExerciseBlocks {
        exercise: Id,
        offset: u32,
        limit: u32,
    },
    SetLock {
        entity: Id,
        revision: u64,
        locked: bool,
    },
    SetAppLock {
        enabled: bool,
    },
    SourceInk {
        block: Id,
    },
    Partition {
        block: Id,
        revision: u64,
        groups: Vec<Vec<Id>>,
        confirmed: bool,
    },
    SplitRegion {
        block: Id,
        revision: u64,
        region: Id,
        parts: Vec<Vec<Id>>,
    },
    MergeRegions {
        block: Id,
        revision: u64,
        regions: Vec<Id>,
    },
    EditRegion {
        block: Id,
        revision: u64,
        region: Id,
        content: Content,
    },
    History {
        block: Id,
    },
    FlatExercises {},
    SearchViews {
        query: String,
    },
    Relock {},
    Status {},
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub command: Command,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub version: u32,
    pub ok: bool,
    pub generation: Option<u64>,
    pub data: Option<Value>,
    pub error: Option<String>,
}
impl Response {
    fn error(e: Error) -> Self {
        Self {
            version: 1,
            ok: false,
            generation: None,
            data: None,
            error: Some(e.to_string()),
        }
    }
}

enum Message {
    PreparePdf(
        PdfSelectionTarget,
        String,
        String,
        Box<dyn PdfGrantVerifier>,
        mpsc::Sender<Result<Value>>,
    ),
    FinishPdf(
        Id,
        bool,
        Box<dyn PdfGrantVerifier>,
        mpsc::Sender<Result<Value>>,
    ),
    Call(Request, mpsc::Sender<Response>),
    Authenticate(
        BTreeSet<Scope>,
        Box<dyn Authenticator + Send>,
        mpsc::Sender<Result<()>>,
    ),
}

/// Admission is separate from completion. Dropping this receipt never cancels an
/// admitted edit. A successful response is the SQLite commit acknowledgement.
pub struct PendingResponse(mpsc::Receiver<Response>);
impl PendingResponse {
    pub fn wait(self) -> Result<Response> {
        self.0.recv().map_err(|_| Error::WorkerFailed)
    }
}
pub struct Host {
    mailbox: Arc<Mailbox>,
    worker: Mutex<Option<thread::JoinHandle<Result<()>>>>,
    writer_thread: thread::ThreadId,
}
impl Host {
    pub fn open(path: PathBuf, capacity: usize) -> Result<Self> {
        Self::open_with_limits(
            path,
            WriterLimits {
                authoring_count: capacity,
                query_count: capacity,
                authoring_bytes: 32 * 1024 * 1024,
                query_bytes: 8 * 1024 * 1024,
            },
        )
    }
    pub fn open_with_limits(path: PathBuf, limits: WriterLimits) -> Result<Self> {
        let mailbox = Mailbox::new(limits)?;
        if path == std::path::Path::new(":memory:") {
            return Err(Error::Invalid("file-backed library path"));
        }
        let ownership = LibraryLease::acquire(&path)?;
        let input = Arc::clone(&mailbox);
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("stem-writer".into())
            .spawn(move || {
                // Outlives Store and all admitted work, including shutdown drain.
                let lease = ownership;
                let mut store = match Store::open(path) {
                    Ok(s) => {
                        let _ = ready_tx.send(Ok(()));
                        s
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        input.close();
                        return Ok(());
                    }
                };
                let mut access = Access::public();
                let mut pdf = PdfState::default();
                while let Ok(Some(envelope)) = input.next() {
                    if access.remaining().is_some_and(|d| d.is_zero()) {
                        access.relock();
                        pdf.clear();
                    }
                    match envelope.message {
                        Message::PreparePdf(target, locator, revision, mut verifier, reply) => {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    pdf.prepare_selection(
                                        &store,
                                        &access,
                                        target,
                                        &locator,
                                        &revision,
                                        verifier.as_mut(),
                                    )
                                }))
                                .unwrap_or(Err(Error::WorkerFailed));
                            let _ =
                                input.complete(envelope.authoring, envelope.bytes, result.is_err());
                            let _ = reply.send(result);
                        }
                        Message::FinishPdf(review, confirmed, mut verifier, reply) => {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    pdf.finish_selection(
                                        &mut store,
                                        &access,
                                        review,
                                        confirmed,
                                        verifier.as_mut(),
                                    )
                                }))
                                .unwrap_or(Err(Error::WorkerFailed));
                            let _ =
                                input.complete(envelope.authoring, envelope.bytes, result.is_err());
                            let _ = reply.send(result);
                        }
                        Message::Authenticate(scopes, mut auth, reply) => {
                            pdf.clear();
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    Access::authenticate(auth.as_mut(), scopes)
                                }))
                                .unwrap_or(Err(Error::WorkerFailed));
                            // A failed verifier must not retain an older grant.
                            access.relock();
                            let result = result.map(|a| {
                                access = a;
                            });
                            let _ =
                                input.complete(envelope.authoring, envelope.bytes, result.is_err());
                            let _ = reply.send(result);
                        }
                        Message::Call(request, reply) => {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    execute(&mut store, &mut access, &mut pdf, request)
                                }))
                                .unwrap_or_else(|_| Response::error(Error::WorkerFailed));
                            let _ = input.complete(envelope.authoring, envelope.bytes, !result.ok);
                            let _ = reply.send(result);
                        }
                    }
                }
                access.relock();
                input.close();
                drop(pdf);
                drop(store);
                lease.release()
            })?;
        match ready_rx.recv() {
            Ok(Ok(())) => {}
            failure => {
                mailbox.close();
                let _ = worker.join();
                return Err(match failure {
                    Ok(Err(error)) => error,
                    _ => Error::WorkerFailed,
                });
            }
        }
        Ok(Self {
            mailbox,
            writer_thread: worker.thread().id(),
            worker: Mutex::new(Some(worker)),
        })
    }
    /// Close admission, drain all accepted work and wait for the writer even when
    /// other callers still hold Arc references. Never call from a verifier callback.
    pub fn close(&self) -> Result<()> {
        self.mailbox.close();
        // Check before taking the join mutex: an external closer may already
        // hold it while waiting for this very verifier to return.
        if self.writer_thread == thread::current().id() {
            return Err(Error::WorkerFailed);
        }
        let mut worker = self.worker.lock().map_err(|_| Error::WorkerFailed)?;
        if let Some(worker) = worker.take() {
            worker.join().map_err(|_| Error::WorkerFailed)??;
        }
        Ok(())
    }
    pub fn subscribe_status(&self) -> StatusSubscription {
        self.mailbox.subscribe()
    }
    pub fn submit(&self, json: &str) -> Result<PendingResponse> {
        if json.len() > 16 * 1024 * 1024 {
            return Err(Error::ResourceLimit);
        }
        let request: Request = serde_json::from_str(json)?;
        let authoring = !matches!(
            &request.command,
            Command::Block { .. }
                | Command::Mode { .. }
                | Command::OpenInMaster { .. }
                | Command::Layout { .. }
                | Command::Group { .. }
                | Command::Embed { .. }
                | Command::View { .. }
                | Command::Links { .. }
                | Command::List { .. }
                | Command::Read { .. }
                | Command::Blocks { .. }
                | Command::Search { .. }
                | Command::Exercises { .. }
                | Command::ExerciseBlocks { .. }
                | Command::SourceInk { .. }
                | Command::History { .. }
                | Command::FlatExercises { .. }
                | Command::SearchViews { .. }
                | Command::Status { .. }
        );
        let authoring = authoring
            && !matches!(&request.command, Command::Pdf { command } if command.is_query());
        let (tx, rx) = mpsc::channel();
        self.mailbox
            .admit(Message::Call(request, tx), authoring, json.len())?;
        Ok(PendingResponse(rx))
    }
    pub fn request(&self, json: &str) -> Result<Response> {
        self.submit(json)?.wait()
    }
    /// Deliberate platform source selection; neither operation is available through JSON.
    pub fn prepare_pdf_selection(
        &self,
        target: PdfSelectionTarget,
        locator: String,
        revision: String,
        verifier: Box<dyn PdfGrantVerifier>,
    ) -> Result<Value> {
        if locator.len() > 4096 || revision.len() > 64 {
            return Err(Error::Invalid("PDF selection"));
        }
        let bytes = 256 + locator.len() + revision.len();
        let (tx, rx) = mpsc::channel();
        self.mailbox.admit(
            Message::PreparePdf(target, locator, revision, verifier, tx),
            false,
            bytes,
        )?;
        rx.recv().map_err(|_| Error::WorkerFailed)?
    }
    pub fn finish_pdf_selection(
        &self,
        review: Id,
        confirmed: bool,
        verifier: Box<dyn PdfGrantVerifier>,
    ) -> Result<Value> {
        let (tx, rx) = mpsc::channel();
        self.mailbox.admit(
            Message::FinishPdf(review, confirmed, verifier, tx),
            true,
            256,
        )?;
        rx.recv().map_err(|_| Error::WorkerFailed)?
    }
    /// Trusted host code supplies a verifier. This is deliberately absent from JSON.
    pub fn authenticate(
        &self,
        scopes: BTreeSet<Scope>,
        auth: Box<dyn Authenticator + Send>,
    ) -> Result<()> {
        if scopes.is_empty() || scopes.len() > 1024 {
            return Err(Error::Invalid("authentication scopes"));
        }
        let bytes = 128 + scopes.len() * 64;
        let (tx, rx) = mpsc::channel();
        self.mailbox
            .admit(Message::Authenticate(scopes, auth, tx), true, bytes)?;
        rx.recv().map_err(|_| Error::WorkerFailed)?
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn execute(s: &mut Store, a: &mut Access, pdf: &mut PdfState, r: Request) -> Response {
    if r.version != 1 {
        return Response::error(Error::Unsupported);
    }
    if a.remaining().is_some_and(|remaining| remaining.is_zero()) {
        a.relock();
        pdf.clear();
    }
    if matches!(
        &r.command,
        Command::Relock {}
            | Command::SetLock { .. }
            | Command::SetBlockLock { .. }
            | Command::ExerciseLock { .. }
            | Command::SetAppLock { .. }
            | Command::Move { .. }
            | Command::Undo { .. }
            | Command::Redo { .. }
    ) {
        pdf.clear();
    }
    let result = (|| -> Result<Value> {
        Ok(match r.command {
            Command::Pdf { command } => pdf.execute(s, a, command)?,
            Command::Block { block } => serde_json::to_value(s.block(block, a)?)?,
            Command::SetMode { entity, mode } => {
                s.set_mode(entity, mode, a)?;
                Value::Null
            }
            Command::Mode { entity } => serde_json::to_value(s.mode(entity, a)?)?,
            Command::AppendTo { target, content } => {
                serde_json::to_value(s.append_content_to(target, content, a)?)?
            }
            Command::CaptureTo {
                ticket,
                target,
                strokes,
            } => serde_json::to_value(s.append_capture(ticket, target, strokes, a)?)?,
            Command::CreateExercise {
                master,
                title,
                blocks,
            } => serde_json::to_value(s.create_exercise(master, &title, &blocks, a)?)?,
            Command::RenameExercise {
                exercise,
                revision,
                title,
            } => serde_json::to_value(s.rename_exercise(exercise, revision, &title, a)?)?,
            Command::ExerciseVisibility {
                exercise,
                revision,
                visible,
            } => serde_json::to_value(s.set_exercise_visible(exercise, revision, visible, a)?)?,
            Command::ExerciseLock {
                exercise,
                revision,
                locked,
            } => serde_json::to_value(s.set_exercise_lock(exercise, revision, locked, a)?)?,
            Command::SetHeading {
                block,
                revision,
                confirmed,
            } => serde_json::to_value(s.set_heading(block, revision, confirmed, a)?)?,
            Command::OpenInMaster { exercise, block } => {
                let location = s.open_in_master(exercise, block, a)?;
                json!({"master": location.master, "block": location.block})
            }
            Command::SetVisible {
                block,
                revision,
                visible,
            } => serde_json::to_value(s.set_visible(block, revision, visible, a)?)?,
            Command::SetBlockLock {
                block,
                revision,
                locked,
            } => serde_json::to_value(s.set_block_lock(block, revision, locked, a)?)?,
            Command::SetDisplay {
                block,
                revision,
                display,
            } => serde_json::to_value(s.set_display(block, revision, display, a)?)?,
            Command::TransformSelection {
                selected,
                transform,
            } => serde_json::to_value(s.transform_selection(&selected, transform, a)?)?,
            Command::EraseStrokes {
                block,
                revision,
                strokes,
            } => serde_json::to_value(s.erase_strokes(block, revision, &strokes, a)?)?,
            Command::RestoreRevision {
                block,
                revision,
                old_revision,
            } => {
                serde_json::to_value(s.restore_block_revision(block, revision, old_revision, a)?)?
            }
            Command::Layout { block } => serde_json::to_value(s.layout(block, a)?)?,
            Command::SetLayout {
                block,
                expected,
                bounds,
                z,
            } => serde_json::to_value(s.set_layout(block, expected, bounds, z, a)?)?,
            Command::EditCell {
                block,
                revision,
                cell,
                value,
            } => serde_json::to_value(s.edit_cell(block, revision, cell, value, a)?)?,
            Command::EditDiagramPart {
                block,
                revision,
                part,
            } => serde_json::to_value(s.edit_diagram_part(block, revision, part, a)?)?,
            Command::CreateGroup { entity, members } => {
                serde_json::to_value(s.create_group(entity, &members, a)?)?
            }
            Command::Group { group } => serde_json::to_value(s.group(group, a)?)?,
            Command::TransformGroup {
                group,
                revision,
                transform,
            } => serde_json::to_value(s.transform_group(group, revision, transform, a)?)?,
            Command::Ungroup { group, revision } => {
                s.ungroup(group, revision, a)?;
                Value::Null
            }
            Command::Embed { embed } => serde_json::to_value(s.embed(embed, a)?)?,
            Command::CreateEmbed {
                host,
                source,
                frame,
            } => serde_json::to_value(s.create_embed(host, source, frame, a)?)?,
            Command::EditEmbed {
                embed,
                revision,
                camera,
                frame,
                pinned,
            } => serde_json::to_value(s.edit_embed(embed, revision, camera, frame, pinned, a)?)?,
            Command::DuplicateEmbed { embed } => {
                serde_json::to_value(s.duplicate_embed(embed, a)?)?
            }
            Command::RemoveEmbed { embed } => {
                s.remove_embed(embed, a)?;
                Value::Null
            }
            Command::View { view } => serde_json::to_value(s.view(view, a)?)?,
            Command::SaveView { view, expected } => {
                serde_json::to_value(s.save_view(view, expected, a)?)?
            }
            Command::AddLink {
                origin,
                target,
                anchor,
            } => serde_json::to_value(s.add_link(origin, target, anchor, a)?)?,
            Command::Links { entity, incoming } => {
                serde_json::to_value(s.links(entity, incoming, a)?)?
            }
            Command::RemoveLink { link } => {
                s.remove_link(link, a)?;
                Value::Null
            }
            Command::List { parent } => {
                serde_json::to_value(s.list(parent, Order::Modified, None, None, a)?)?
            }
            Command::Create {
                kind,
                parent,
                title,
            } => serde_json::to_value(s.create(kind, parent, &title, a)?)?,
            Command::Read { id } => serde_json::to_value(s.entity(id, a)?)?,
            Command::Blocks {
                entity,
                offset,
                limit,
            } => serde_json::to_value(s.blocks(entity, offset, limit, a)?)?,
            Command::Append { entity, content } => {
                serde_json::to_value(s.append_content(entity, content, a)?)?
            }
            Command::Capture {
                ticket,
                entity,
                strokes,
            } => serde_json::to_value(s.append_capture(
                ticket,
                WritingTarget::Entity(entity),
                strokes,
                a,
            )?)?,
            Command::Edit {
                block,
                revision,
                content,
            } => serde_json::to_value(s.edit_content(block, revision, content, a)?)?,
            Command::Rename {
                entity,
                revision,
                title,
            } => serde_json::to_value(s.rename(entity, revision, &title, a)?)?,
            Command::Move {
                entity,
                revision,
                parent,
            } => serde_json::to_value(s.move_entity(entity, revision, parent, a)?)?,
            Command::Undo { entity } => json!(s.undo(entity, a)?),
            Command::Redo { entity } => json!(s.redo(entity, a)?),
            Command::Search { query } => serde_json::to_value(s.search(&query, a)?)?,
            Command::Exercises { master } => serde_json::to_value(s.exercises(master, false, a)?)?,
            Command::ExerciseBlocks {
                exercise,
                offset,
                limit,
            } => serde_json::to_value(s.exercise_blocks(exercise, offset, limit, a)?)?,
            Command::SetLock {
                entity,
                revision,
                locked,
            } => serde_json::to_value(s.set_lock(entity, revision, locked, a)?)?,
            Command::SetAppLock { enabled } => {
                s.set_app_lock(enabled, a)?;
                Value::Null
            }
            Command::SourceInk { block } => serde_json::to_value(s.source_ink(block, a)?)?,
            Command::Partition {
                block,
                revision,
                groups,
                confirmed,
            } => serde_json::to_value(s.partition_regions(block, revision, groups, confirmed, a)?)?,
            Command::SplitRegion {
                block,
                revision,
                region,
                parts,
            } => serde_json::to_value(s.split_region(block, revision, region, parts, a)?)?,
            Command::MergeRegions {
                block,
                revision,
                regions,
            } => serde_json::to_value(s.merge_regions(block, revision, &regions, a)?)?,
            Command::EditRegion {
                block,
                revision,
                region,
                content,
            } => serde_json::to_value(s.edit_region(block, revision, region, content, a)?)?,
            Command::History { block } => serde_json::to_value(s.history(block, a)?)?,
            Command::FlatExercises {} => serde_json::to_value(s.flat_exercises(a)?)?,
            Command::SearchViews { query } => serde_json::to_value(s.search_views(&query, a)?)?,
            Command::Relock {} => {
                a.relock();
                Value::Null
            }
            Command::Status {} => {
                Store::require_app(&s.db, a)?;
                json!({"schema":s.schema_version()?,"saved":s.save_state()==crate::store::SaveState::SavedLocally})
            }
        })
    })();
    match result {
        Ok(data) => Response {
            version: 1,
            ok: true,
            generation: Store::require_app(&s.db, a)
                .ok()
                .and_then(|_| Store::setting(&s.db, "generation").ok()?.parse().ok()),
            data: Some(data),
            error: None,
        },
        Err(e) => Response::error(e),
    }
}
