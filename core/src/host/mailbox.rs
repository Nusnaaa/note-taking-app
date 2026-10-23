//! Bounded authoring/query admission and coalesced content-free status.
use super::Message;
use crate::{Error, Result};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::Duration,
};

#[derive(Debug, Clone, Copy)]
pub struct WriterLimits {
    pub authoring_count: usize,
    pub authoring_bytes: usize,
    pub query_count: usize,
    pub query_bytes: usize,
}
impl WriterLimits {
    pub(super) fn validate(self) -> Result<Self> {
        if !(1..=4096).contains(&self.authoring_count)
            || !(1..=4096).contains(&self.query_count)
            || !(1..=64 * 1024 * 1024).contains(&self.authoring_bytes)
            || !(1..=64 * 1024 * 1024).contains(&self.query_bytes)
        {
            return Err(Error::Invalid("writer capacity"));
        }
        Ok(self)
    }
}

/// These are host admission counters, not document data or a durable-save receipt.
/// Attention is sticky for this host lifetime after an admitted authoring failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HostStatus {
    pub sequence: u64,
    pub pending_authoring: usize,
    pub pending_queries: usize,
    pub needs_attention: bool,
    pub closed: bool,
}

pub(super) struct Envelope {
    pub message: Message,
    pub authoring: bool,
    pub bytes: usize,
}
struct State {
    authoring: VecDeque<Envelope>,
    queries: VecDeque<Envelope>,
    authoring_bytes: usize,
    query_bytes: usize,
    status: HostStatus,
}
pub(super) struct Mailbox {
    state: Mutex<State>,
    changed: Condvar,
    limits: WriterLimits,
}
impl Mailbox {
    pub fn new(limits: WriterLimits) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            limits: limits.validate()?,
            state: Mutex::new(State {
                authoring: VecDeque::new(),
                queries: VecDeque::new(),
                authoring_bytes: 0,
                query_bytes: 0,
                status: HostStatus {
                    sequence: 0,
                    pending_authoring: 0,
                    pending_queries: 0,
                    needs_attention: false,
                    closed: false,
                },
            }),
            changed: Condvar::new(),
        }))
    }
    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| Error::WorkerFailed)
    }
    fn publish(&self, s: &mut State) {
        s.status.sequence = s.status.sequence.wrapping_add(1);
        self.changed.notify_all();
    }
    pub fn admit(&self, message: Message, authoring: bool, bytes: usize) -> Result<()> {
        let mut s = self.lock()?;
        if s.status.closed {
            return Err(Error::WorkerFailed);
        }
        let (count, used, count_limit, byte_limit) = if authoring {
            (
                s.status.pending_authoring,
                s.authoring_bytes,
                self.limits.authoring_count,
                self.limits.authoring_bytes,
            )
        } else {
            (
                s.status.pending_queries,
                s.query_bytes,
                self.limits.query_count,
                self.limits.query_bytes,
            )
        };
        if count >= count_limit || bytes > byte_limit.saturating_sub(used) {
            return Err(Error::ResourceLimit);
        }
        let envelope = Envelope {
            message,
            authoring,
            bytes,
        };
        if authoring {
            s.authoring.push_back(envelope);
            s.status.pending_authoring += 1;
            s.authoring_bytes += bytes;
        } else {
            s.queries.push_back(envelope);
            s.status.pending_queries += 1;
            s.query_bytes += bytes;
        }
        self.publish(&mut s);
        Ok(())
    }
    pub fn next(&self) -> Result<Option<Envelope>> {
        let mut s = self.lock()?;
        loop {
            // Preserve FIFO among all edits, capture, undo and access changes.
            // A running query is not preempted; queued optional reads yield.
            if let Some(e) = s.authoring.pop_front().or_else(|| s.queries.pop_front()) {
                return Ok(Some(e));
            }
            if s.status.closed {
                return Ok(None);
            }
            s = self.changed.wait(s).map_err(|_| Error::WorkerFailed)?;
        }
    }
    pub fn complete(&self, authoring: bool, bytes: usize, failed: bool) -> Result<()> {
        let mut s = self.lock()?;
        if authoring {
            s.status.pending_authoring -= 1;
            s.authoring_bytes -= bytes;
            s.status.needs_attention |= failed;
        } else {
            s.status.pending_queries -= 1;
            s.query_bytes -= bytes;
        }
        self.publish(&mut s);
        Ok(())
    }
    pub fn close(&self) {
        if let Ok(mut s) = self.lock() {
            s.status.closed = true;
            self.publish(&mut s);
        }
    }
    pub fn subscribe(self: &Arc<Self>) -> StatusSubscription {
        StatusSubscription {
            mailbox: Arc::clone(self),
            seen: None,
        }
    }
}

/// Pull-based coalescing: no callbacks or per-observer event queue run on the writer.
/// A slow observer reads the latest snapshot. Closing never reopens the mailbox.
pub struct StatusSubscription {
    mailbox: Arc<Mailbox>,
    seen: Option<u64>,
}
impl StatusSubscription {
    pub fn next_timeout(&mut self, timeout: Duration) -> Result<Option<HostStatus>> {
        let s = self.mailbox.lock()?;
        let (s, _) = self
            .mailbox
            .changed
            .wait_timeout_while(s, timeout, |s| {
                self.seen == Some(s.status.sequence) && !s.status.closed
            })
            .map_err(|_| Error::WorkerFailed)?;
        if self.seen == Some(s.status.sequence) {
            return Ok(None);
        }
        let status = s.status;
        self.seen = Some(status.sequence);
        Ok(Some(status))
    }
}
