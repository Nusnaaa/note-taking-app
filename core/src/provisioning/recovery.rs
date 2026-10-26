//! Read-only transfer discovery. Never call resume here: resume truncates the
//! unacknowledged tail. Completion flags are provenance, not hash verification.
use super::*;
use std::collections::BTreeMap;
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum RecoveryState {
    Interrupted,
    RecordedComplete,
    ReviewRequired,
    Invalid,
}
#[derive(Debug, Clone, Serialize)]
pub struct RecoveryEntry {
    pub id: Id,
    pub state: RecoveryState,
    pub descriptor: Option<Descriptor>,
    pub total: Option<u64>,
    pub committed: Option<u64>,
    pub consent_ms: Option<i64>,
}
#[derive(Debug, Clone, Serialize)]
pub struct RecoveryPage {
    pub entries: Vec<RecoveryEntry>,
    pub next: Option<Id>,
}
fn redacted(id: Id, state: RecoveryState) -> RecoveryEntry {
    RecoveryEntry {
        id,
        state,
        descriptor: None,
        total: None,
        committed: None,
        consent_ms: None,
    }
}
fn inspect(root: &Path, id: Id, library: &str) -> Result<Option<RecoveryEntry>> {
    let path = root.join(format!("{id}.install.json"));
    let meta = std::fs::symlink_metadata(&path)?;
    if !meta.file_type().is_file() || meta.len() > 65536 {
        return Ok(Some(redacted(id, RecoveryState::Invalid)));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Ok(Some(redacted(id, RecoveryState::Invalid)));
    }
    let Ok(state) = serde_json::from_slice::<State>(&bytes) else {
        return Ok(Some(redacted(id, RecoveryState::Invalid)));
    };
    let Some(binding) = &state.binding else {
        return Ok(Some(redacted(id, RecoveryState::ReviewRequired)));
    };
    if binding.library != library {
        return Ok(None);
    }
    if state.id != id
        || state.committed > state.total
        || (state.complete && state.committed != state.total)
        || Transfer::validate_logical_manifest(&state.descriptor, state.total).is_err()
    {
        return Ok(Some(redacted(id, RecoveryState::Invalid)));
    }
    let part = std::fs::symlink_metadata(root.join(format!("{id}.part")));
    if !part.is_ok_and(|m| m.file_type().is_file() && m.len() >= state.committed) {
        return Ok(Some(redacted(id, RecoveryState::Invalid)));
    }
    if state.complete
        && !std::fs::symlink_metadata(root.join(format!("{id}.resource")))
            .is_ok_and(|m| m.file_type().is_file() && m.len() == state.total)
    {
        return Ok(Some(redacted(id, RecoveryState::Invalid)));
    }
    Ok(Some(RecoveryEntry {
        id,
        state: if state.complete {
            RecoveryState::RecordedComplete
        } else {
            RecoveryState::Interrupted
        },
        descriptor: Some(state.descriptor),
        total: Some(state.total),
        committed: Some(state.committed),
        consent_ms: Some(binding.consent_ms),
    }))
}
pub(crate) fn scan(
    root: &Path,
    library: &str,
    after: Option<Id>,
    limit: usize,
    mut check: impl FnMut() -> Result<()>,
) -> Result<RecoveryPage> {
    check()?;
    if !(1..=128).contains(&limit) {
        return Err(Error::ResourceLimit);
    }
    let meta = match std::fs::symlink_metadata(root) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            check()?;
            return Ok(RecoveryPage {
                entries: Vec::new(),
                next: None,
            });
        }
        Err(e) => return Err(e.into()),
    };
    if !meta.file_type().is_dir() {
        return Err(Error::Integrity);
    }
    // Keep only the smallest limit+1 eligible IDs. Directory size never grows
    // memory; cancellation is checked per entry and around bounded file reads.
    let mut selected = BTreeMap::new();
    for item in std::fs::read_dir(root)? {
        check()?;
        let item = item?;
        let name = item.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(raw) = name.strip_suffix(".install.json") else {
            continue;
        };
        let Ok(id) = Id::parse(raw) else {
            continue;
        };
        if raw != id.to_string()
            || after.is_some_and(|after| id <= after)
            || (selected.len() == limit + 1
                && selected
                    .last_key_value()
                    .is_some_and(|(last, _)| id >= *last))
        {
            continue;
        }
        let entry = inspect(root, id, library)?;
        check()?;
        if let Some(entry) = entry {
            selected.insert(id, entry);
            if selected.len() > limit + 1 {
                selected.pop_last();
            }
        }
    }
    let mut entries = Vec::new();
    let mut bytes = 0usize;
    let mut next = None;
    for entry in selected.into_values() {
        let charge = serde_json::to_vec(&entry)?.len();
        if entries.len() == limit || bytes > (4 * 1024 * 1024usize).saturating_sub(charge) {
            next = entries.last().map(|entry: &RecoveryEntry| entry.id);
            break;
        }
        bytes += charge;
        entries.push(entry);
    }
    check()?;
    Ok(RecoveryPage { entries, next })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_rechecks_control_between_entries_without_changing_files() {
        let root = tempfile::tempdir().unwrap();
        for _ in 0..4 {
            std::fs::write(
                root.path().join(format!("{}.install.json", Id::new())),
                b"invalid",
            )
            .unwrap();
        }
        let mut checks = 0;
        let result = scan(root.path(), "library", None, 128, || {
            checks += 1;
            if checks == 4 {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(Error::Cancelled)));
        assert_eq!(checks, 4);
        for entry in std::fs::read_dir(root.path()).unwrap() {
            assert_eq!(std::fs::read(entry.unwrap().path()).unwrap(), b"invalid");
        }
    }
}
