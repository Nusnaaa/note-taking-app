//! Versioned JSON boundary used by the native host and developer harness.
//! The bounded mailbox owns the single writer. Callers retain rejected requests;
//! only a successful response acknowledges a committed edit. Never call on UI thread.
use crate::{
    content::Content,
    ink::Stroke,
    security::{Access, Authenticator, Scope},
    store::{Kind, Order, Store, WritingTarget},
    Error, Id, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::PathBuf, sync::mpsc, thread};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum Command {
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
    Call(Request, mpsc::Sender<Response>),
    Authenticate(
        BTreeSet<Scope>,
        Box<dyn Authenticator + Send>,
        mpsc::Sender<Result<()>>,
    ),
    Close,
}
pub struct Host {
    tx: mpsc::SyncSender<Message>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Host {
    pub fn open(path: PathBuf, capacity: usize) -> Result<Self> {
        if capacity == 0 || capacity > 4096 {
            return Err(Error::Invalid("writer capacity"));
        }
        let (tx, rx) = mpsc::sync_channel(capacity);
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("stem-writer".into())
            .spawn(move || {
                let mut store = match Store::open(path) {
                    Ok(s) => {
                        let _ = ready_tx.send(Ok(()));
                        s
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let mut access = Access::public();
                while let Ok(message) = rx.recv() {
                    match message {
                        Message::Close => break,
                        Message::Authenticate(scopes, mut auth, reply) => {
                            let result = Access::authenticate(auth.as_mut(), scopes).map(|a| {
                                access = a;
                            });
                            let _ = reply.send(result);
                        }
                        Message::Call(request, reply) => {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    execute(&mut store, &mut access, request)
                                }))
                                .unwrap_or_else(|_| Response::error(Error::WorkerFailed));
                            let _ = reply.send(result);
                        }
                    }
                }
                access.relock();
            })?;
        ready_rx.recv().map_err(|_| Error::WorkerFailed)??;
        Ok(Self {
            tx,
            worker: Some(worker),
        })
    }
    pub fn request(&self, json: &str) -> Result<Response> {
        if json.len() > 16 * 1024 * 1024 {
            return Err(Error::ResourceLimit);
        }
        let request: Request = serde_json::from_str(json)?;
        let (tx, rx) = mpsc::channel();
        self.tx
            .try_send(Message::Call(request, tx))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Error::ResourceLimit,
                _ => Error::WorkerFailed,
            })?;
        rx.recv().map_err(|_| Error::WorkerFailed)
    }
    /// Trusted host code supplies a verifier. This is deliberately absent from JSON.
    pub fn authenticate(
        &self,
        scopes: BTreeSet<Scope>,
        auth: Box<dyn Authenticator + Send>,
    ) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .try_send(Message::Authenticate(scopes, auth, tx))
            .map_err(|_| Error::ResourceLimit)?;
        rx.recv().map_err(|_| Error::WorkerFailed)?
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.tx.send(Message::Close);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn execute(s: &mut Store, a: &mut Access, r: Request) -> Response {
    if r.version != 1 {
        return Response::error(Error::Unsupported);
    }
    let result = (|| -> Result<Value> {
        Ok(match r.command {
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
