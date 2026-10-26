mod common;
use common::*;
use std::time::Duration;
use stem_core::{
    content::Content,
    objects::{Cell, CellValue, Table},
    processing::*,
    security::Access,
    store::{Block, FailurePoint, InkSnapshot, Kind, Store},
    Error, Id, Result,
};
fn fixture() -> (Store, Access, Id, Block, Id, Id) {
    let mut store = Store::memory().unwrap();
    let access = Access::public();
    let note = store.create(Kind::Note, None, "cell ink", &access).unwrap();
    let first = Id::new();
    let second = Id::new();
    let block = store
        .append_content(
            note.id,
            Content::Table(Table {
                rows: vec![vec![
                    Cell {
                        id: first,
                        value: CellValue::Text("old".into()),
                    },
                    Cell {
                        id: second,
                        value: CellValue::Numeric("001".into()),
                    },
                ]],
                header_rows: 0,
            }),
            &access,
        )
        .unwrap();
    (store, access, note.id, block, first, second)
}
fn capture(store: &mut Store, access: &Access, block: &Block, cell: Id, sequence: u64) -> Block {
    store
        .append_cell_capture(
            Id::new(),
            block.id,
            block.revision,
            cell,
            vec![stroke(sequence)],
            access,
        )
        .unwrap()
}
struct Echo;
impl Recogniser for Echo {
    fn probe(&self) -> Result<()> {
        Ok(())
    }
    fn recognise(
        &self,
        snapshot: &InkSnapshot,
        _: Task,
        _: &JobControl,
    ) -> Result<RecognitionOutput> {
        assert_eq!(snapshot.strokes().len(), 1);
        Ok(RecognitionOutput {
            candidates: vec![Content::Text("1 + 1 = 3".into())],
            raw_output: "1 + 1 = 3".into(),
            confidence: None,
            diagnostics: vec![],
        })
    }
}
fn manager() -> Manager {
    let mut manager = Manager::new();
    manager
        .register_recogniser(
            Descriptor {
                id: "cell-fixture".into(),
                version: "1".into(),
                adapter_version: "1".into(),
                family: Family::Recogniser,
                input: InputContract::OrderedStrokesV1,
                tasks: vec![Task::Prose],
                internals: Internals::StrokeOnlyVerified,
                offline: true,
                content_egress: false,
                personal_use_evidence: Some("app-owned synthetic fixture".into()),
                redistributable_resources: false,
                resource_sha256: None,
                runtime_contract: "stem-processing-v1".into(),
            },
            Box::new(Echo),
        )
        .unwrap();
    manager.activate_local("cell-fixture", None, true).unwrap();
    manager.select(Task::Prose, "cell-fixture").unwrap();
    manager
}
fn propose(store: &mut Store, access: &Access, block: Id, cell: Id) -> stem_core::store::Proposal {
    let snapshot = store.region_snapshot(block, cell, access).unwrap();
    let completed = manager()
        .recognise(
            snapshot,
            Task::Prose,
            None,
            &JobControl::new(Duration::from_secs(30)).unwrap(),
        )
        .unwrap();
    store.propose(completed, access).unwrap()
}
#[test]
fn capture_is_idempotent_scoped_atomic_and_undo_preserves_observations() {
    let (mut store, access, note, block, first, second) = fixture();
    let ticket = Id::new();
    let ink = vec![stroke(0)];
    let captured = store
        .append_cell_capture(ticket, block.id, 0, first, ink.clone(), &access)
        .unwrap();
    assert_eq!(captured.revision, 1);
    assert_eq!(captured.regions[0].id, first);
    assert_eq!(
        store
            .append_cell_capture(ticket, block.id, 0, first, ink.clone(), &access)
            .unwrap(),
        captured
    );
    assert!(matches!(
        store.append_cell_capture(ticket, block.id, 0, second, ink.clone(), &access),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        store.ink_snapshot(block.id, &access),
        Err(Error::ReviewRequired)
    ));
    store.undo(note, &access).unwrap();
    let undone = store.block(block.id, &access).unwrap();
    assert!(undone.regions.is_empty());
    assert_eq!(store.source_ink(block.id, &access).unwrap(), ink);
    store.redo(note, &access).unwrap();
    assert_eq!(
        store.block(block.id, &access).unwrap().regions,
        captured.regions
    );
}
#[test]
fn independent_cell_review_updates_only_chosen_cell_and_stale_proposals_fail() {
    let (mut store, access, _, block, first, second) = fixture();
    let block = capture(&mut store, &access, &block, first, 0);
    let block = capture(&mut store, &access, &block, second, 1);
    let first_proposal = propose(&mut store, &access, block.id, first);
    let stale = propose(&mut store, &access, block.id, second);
    let accepted = store
        .accept_proposal(first_proposal.id, 0, &access)
        .unwrap();
    let Content::Table(table) = &accepted.content else {
        panic!()
    };
    assert_eq!(table.rows[0][0].value, CellValue::Text("1 + 1 = 3".into()));
    assert_eq!(table.rows[0][1].value, CellValue::Numeric("001".into()));
    assert!(accepted
        .regions
        .iter()
        .find(|r| r.id == second)
        .unwrap()
        .content
        .is_none());
    assert!(matches!(
        store.accept_proposal(stale.id, 0, &access),
        Err(Error::Conflict)
    ));
    assert_eq!(store.source_ink(block.id, &access).unwrap().len(), 2);
}
#[test]
fn manual_cell_correction_is_independent_and_new_ink_never_rewrites_it() {
    let (mut store, access, _, block, first, _) = fixture();
    let block = capture(&mut store, &access, &block, first, 0);
    let proposal = propose(&mut store, &access, block.id, first);
    let block = store
        .edit_cell(
            block.id,
            block.revision,
            first,
            CellValue::Text("deliberate error".into()),
            &access,
        )
        .unwrap();
    assert!(matches!(
        store.accept_proposal(proposal.id, 0, &access),
        Err(Error::Conflict)
    ));
    let block = capture(&mut store, &access, &block, first, 1);
    let Content::Table(table) = &block.content else {
        panic!()
    };
    assert_eq!(
        table.rows[0][0].value,
        CellValue::Text("deliberate error".into())
    );
    assert!(block.regions[0].content.is_none());
    assert_eq!(block.source_ids.len(), 2);
}
#[test]
fn invalid_target_identity_and_injected_commit_failure_leave_source_unchanged() {
    let (mut store, access, _, block, first, _) = fixture();
    assert!(matches!(
        store.append_cell_capture(Id::new(), block.id, 0, Id::new(), vec![stroke(0)], &access),
        Err(Error::Unavailable)
    ));
    store.inject_failure(FailurePoint::BeforeCommit);
    assert!(matches!(
        store.append_cell_capture(Id::new(), block.id, 0, first, vec![stroke(1)], &access),
        Err(Error::Interrupted)
    ));
    assert_eq!(store.block(block.id, &access).unwrap(), block);
    assert!(store.source_ink(block.id, &access).unwrap().is_empty());
}
#[test]
fn native_copy_preserves_cell_binding_with_independent_ids_and_refuses_legacy_tag() {
    use stem_core::portable::{ExportProtection, NativeDocument};
    let (mut store, access, note, block, first, _) = fixture();
    let block = capture(&mut store, &access, &block, first, 0);
    let plan = store.prepare_native_export(&[note], true, &access).unwrap();
    let bytes = store
        .finish_native_export(
            plan,
            ExportProtection::Plaintext {
                acknowledge_protected: true,
            },
            &mut TestAuth(true),
            true,
        )
        .unwrap()
        .bytes;
    let mut document = NativeDocument::decode(&bytes).unwrap();
    assert_eq!(document.version, 11);
    document.version = 10;
    assert!(matches!(document.validate(), Err(Error::Unsupported)));
    let mut copy = Store::memory().unwrap();
    let ids = copy.import_native(&bytes, &access).unwrap();
    let copied = copy.blocks(ids[0], 0, 10, &access).unwrap().remove(0);
    let Content::Table(table) = &copied.content else {
        panic!()
    };
    assert_ne!(table.rows[0][0].id, first);
    assert_eq!(copied.regions[0].id, table.rows[0][0].id);
    assert_ne!(copied.regions[0].strokes, block.regions[0].strokes);
    assert!(copy
        .region_snapshot(copied.id, copied.regions[0].id, &access)
        .is_ok());
}

#[test]
fn classic_backup_preserves_cells_ink_and_rejects_legacy_tags_even_after_undo() {
    use stem_core::{backup::BackupOptions, crypto};
    let (mut store, access, note, block, first, _) = fixture();
    let block = capture(&mut store, &access, &block, first, 0);
    let proposal = propose(&mut store, &access, block.id, first);
    store.accept_proposal(proposal.id, 0, &access).unwrap();
    store.undo(note, &access).unwrap();
    store.undo(note, &access).unwrap();
    let options = BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    };
    let password = "synthetic table cell backup password";
    let backup = store
        .finish_backup(
            store.prepare_backup(options, &access).unwrap(),
            password,
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    let restored = Store::restore_backup_password(&backup.bytes, password).unwrap();
    assert_eq!(restored.schema_version().unwrap(), 19);
    assert_eq!(
        restored.source_ink(block.id, &access).unwrap(),
        store.source_ink(block.id, &access).unwrap()
    );
    let plain = crypto::decrypt(&backup.bytes, crypto::Credential::Password(password)).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    assert_eq!(json["schema"], 19);
    json["schema"] = 18.into();
    let disguised = crypto::encrypt(&serde_json::to_vec(&json).unwrap(), password).unwrap();
    assert!(matches!(
        Store::restore_backup_password(&disguised.bytes, password),
        Err(Error::Unsupported)
    ));
}

#[test]
fn streaming_backup_preserves_cell_targets_and_blocks_legacy_schema_smuggling() {
    use stem_core::{backup::BackupOptions, crypto};
    let (mut store, access, _, block, first, _) = fixture();
    let block = capture(&mut store, &access, &block, first, 0);
    let options = BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    };
    let password = "synthetic table cell streaming password";
    let control = JobControl::new(Duration::from_secs(180)).unwrap();
    let (backup, _) = store
        .finish_stream_backup(
            store.prepare_stream_backup(options, &access).unwrap(),
            password,
            &mut TestAuth(true),
            true,
            Vec::new(),
            &control,
        )
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let restored = Store::restore_stream_backup(
        backup.as_slice(),
        crypto::Credential::Password(password),
        16 * 1024 * 1024,
        root.path(),
        &control,
    )
    .unwrap();
    assert_eq!(
        restored.store().block(block.id, &access).unwrap().regions,
        block.regions
    );
    use std::io::{Read, Write};
    let mut reader = crypto::stream::DecryptStream::new(
        backup.as_slice(),
        crypto::Credential::Password(password),
        16 * 1024 * 1024,
    )
    .unwrap();
    let mut plain = Vec::new();
    reader.read_to_end(&mut plain).unwrap();
    reader.finish().unwrap();
    plain[8..12].copy_from_slice(&18u32.to_be_bytes());
    let mut writer = crypto::stream::EncryptStream::new(Vec::new(), password).unwrap();
    writer.write_all(&plain).unwrap();
    let disguised = writer.finish().unwrap();
    assert!(matches!(
        Store::restore_stream_backup(
            disguised.0.as_slice(),
            crypto::Credential::Password(password),
            16 * 1024 * 1024,
            root.path(),
            &control
        ),
        Err(Error::Unsupported)
    ));
}

#[test]
fn duplicate_cell_targets_across_tables_and_fresh_protection_fail_closed() {
    use stem_core::security::Scope;
    let (mut store, access, note, block, first, _) = fixture();
    let duplicate = store
        .append_content(note, block.content.clone(), &access)
        .unwrap();
    let _captured = capture(&mut store, &access, &block, first, 0);
    assert!(matches!(
        store.append_cell_capture(Id::new(), duplicate.id, 0, first, vec![stroke(1)], &access),
        Err(Error::Conflict)
    ));
    let scoped = common::access([Scope::Block(duplicate.id)]);
    let locked = store
        .set_block_lock(duplicate.id, 0, true, &scoped)
        .unwrap();
    assert!(matches!(
        store.append_cell_capture(
            Id::new(),
            locked.id,
            locked.revision,
            first,
            vec![stroke(2)],
            &access
        ),
        Err(Error::Denied)
    ));
    assert!(store.source_ink(duplicate.id, &scoped).unwrap().is_empty());
}

#[test]
fn derivative_exports_cell_text_once_and_discloses_unconverted_ink_omission() {
    use stem_core::export::Selection;
    let (mut store, access, note, block, first, _) = fixture();
    let block = capture(&mut store, &access, &block, first, 0);
    let plan = store
        .prepare_derivative(Selection::Document(note), false, &access)
        .unwrap();
    assert!(plan
        .warnings
        .iter()
        .any(|warning| warning.contains("table-cell")));
    let proposal = propose(&mut store, &access, block.id, first);
    store.accept_proposal(proposal.id, 0, &access).unwrap();
    let plan = store
        .prepare_derivative(Selection::Document(note), false, &access)
        .unwrap();
    let bundle = store
        .finish_derivative(plan, &mut TestAuth(true), true)
        .unwrap();
    assert_eq!(bundle.markdown.matches("1 + 1 = 3").count(), 1);
    assert!(!bundle.includes_original_ink);
}

#[test]
fn incremental_capture_preserves_session_order_and_acknowledges_exact_replay() {
    let (mut store, access, _, block, first, _) = fixture();
    let initial = stroke(7);
    let ticket = Id::new();
    let captured = store
        .append_cell_capture(ticket, block.id, 0, first, vec![initial.clone()], &access)
        .unwrap();
    assert!(store
        .append_cell_capture(ticket, block.id, 0, first, vec![initial.clone()], &access)
        .is_ok());
    let mut backwards = stroke(6);
    backwards.capture_session = initial.capture_session;
    assert!(matches!(
        store.append_cell_capture(
            Id::new(),
            block.id,
            captured.revision,
            first,
            vec![backwards],
            &access
        ),
        Err(Error::Invalid("cell capture session order"))
    ));
    assert_eq!(store.block(block.id, &access).unwrap(), captured);
    let mut next = stroke(8);
    next.capture_session = initial.capture_session;
    let final_block = store
        .append_cell_capture(
            Id::new(),
            block.id,
            captured.revision,
            first,
            vec![next.clone()],
            &access,
        )
        .unwrap();
    assert_eq!(
        store
            .region_snapshot(block.id, first, &access)
            .unwrap()
            .strokes(),
        &[initial, next]
    );
    assert_eq!(final_block.revision, 2);
}

#[test]
fn host_capture_and_scoped_cell_proposal_preserve_neighbouring_values() {
    use stem_core::host::{Command, Host, Request};
    fn call(host: &Host, command: Command) -> serde_json::Value {
        let response = host
            .request(
                &serde_json::to_string(&Request {
                    version: 1,
                    command,
                })
                .unwrap(),
            )
            .unwrap();
        assert!(response.ok, "{:?}", response.error);
        response.data.unwrap()
    }
    let root = tempfile::tempdir().unwrap();
    let host = Host::open(root.path().join("library.db"), 8).unwrap();
    let first = Id::new();
    let second = Id::new();
    let note = Id::parse(
        call(
            &host,
            Command::Create {
                kind: Kind::Note,
                parent: None,
                title: "host cells".into(),
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let block = Id::parse(
        call(
            &host,
            Command::Append {
                entity: note,
                content: Content::Table(Table {
                    rows: vec![vec![
                        Cell {
                            id: first,
                            value: CellValue::Text("old".into()),
                        },
                        Cell {
                            id: second,
                            value: CellValue::Numeric("001".into()),
                        },
                    ]],
                    header_rows: 0,
                }),
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    call(
        &host,
        Command::CaptureCell {
            ticket: Id::new(),
            block,
            revision: 0,
            cell: first,
            strokes: vec![stroke(0)],
        },
    );
    let manager = manager();
    let completed = host
        .recognition_source(block, Some(first))
        .unwrap()
        .prepare(&manager, Task::Prose, None)
        .unwrap()
        .run(&JobControl::new(Duration::from_secs(30)).unwrap())
        .unwrap();
    let proposal = host.publish_recognition(completed).unwrap();
    assert_eq!(proposal.region, Some(first));
    let accepted: Block = serde_json::from_value(call(
        &host,
        Command::AcceptProposal {
            proposal: proposal.id,
            candidate: 0,
        },
    ))
    .unwrap();
    let Content::Table(table) = &accepted.content else {
        panic!()
    };
    assert_eq!(table.rows[0][0].value, CellValue::Text("1 + 1 = 3".into()));
    assert_eq!(table.rows[0][1].value, CellValue::Numeric("001".into()));
    // The source remains separately discoverable through the public native host.
    assert_eq!(
        call(&host, Command::SourceInk { block })
            .as_array()
            .unwrap()
            .len(),
        1
    );
    host.close().unwrap();
}

#[test]
fn independent_table_copy_remaps_saved_cell_cursor_with_its_target() {
    use stem_core::{
        portable::{ExportProtection, NativeDocument},
        store::{
            BookmarkState, Camera, CaretPoint, CaretTarget, InlinePoint, SelectionBookmark,
            ViewSettings,
        },
    };
    let (mut store, access, note, block, first, _) = fixture();
    let block = capture(&mut store, &access, &block, first, 0);
    let point = CaretPoint {
        block: block.id,
        revision: block.revision,
        region: None,
        target: CaretTarget::Cell {
            cell: first,
            point: InlinePoint::Text { offset: 1 },
        },
    };
    store
        .save_view(
            ViewSettings {
                id: Id::new(),
                entity: note,
                exercise: None,
                camera: Camera::default(),
                pen_width_mm: 0.5,
                colour_rgba: [0, 0, 0, 255],
                finger_ink: false,
                anchor: None,
                bookmark: Some(SelectionBookmark {
                    anchor: point.clone(),
                    focus: point,
                }),
                authoring: None,
                revision: 0,
            },
            None,
            &access,
        )
        .unwrap();
    let bytes = store
        .finish_native_export(
            store.prepare_native_export(&[note], true, &access).unwrap(),
            ExportProtection::Plaintext {
                acknowledge_protected: true,
            },
            &mut TestAuth(true),
            true,
        )
        .unwrap()
        .bytes;
    let mut target = Store::memory().unwrap();
    let roots = target.import_native(&bytes, &access).unwrap();
    let copied = target
        .finish_native_export(
            target.prepare_native_export(&roots, true, &access).unwrap(),
            ExportProtection::Plaintext {
                acknowledge_protected: true,
            },
            &mut TestAuth(true),
            true,
        )
        .unwrap()
        .bytes;
    let doc = NativeDocument::decode(&copied).unwrap();
    let view = &doc.attachments.views[0];
    let selected = &view.bookmark.as_ref().unwrap().anchor;
    let CaretTarget::Cell { cell, .. } = selected.target else {
        panic!()
    };
    let Content::Table(table) = &doc.blocks[0].current.content else {
        panic!()
    };
    assert_ne!(cell, first);
    assert_eq!(cell, table.rows[0][0].id);
    assert_eq!(doc.blocks[0].current.regions[0].id, cell);
    assert_eq!(
        target
            .restore_view(view.id, &access)
            .unwrap()
            .bookmark_state,
        BookmarkState::Current
    );
}

#[test]
fn accepting_another_cell_retains_existing_manual_correction_and_marker() {
    let (mut store, access, _, block, first, second) = fixture();
    let block = capture(&mut store, &access, &block, first, 0);
    let block = capture(&mut store, &access, &block, second, 1);
    store
        .edit_cell(
            block.id,
            block.revision,
            first,
            CellValue::Text("author deliberately wrote this".into()),
            &access,
        )
        .unwrap();
    let proposal = propose(&mut store, &access, block.id, second);
    let accepted = store.accept_proposal(proposal.id, 0, &access).unwrap();
    assert!(accepted.manually_corrected);
    let Content::Table(table) = accepted.content else {
        panic!()
    };
    assert_eq!(
        table.rows[0][0].value,
        CellValue::Text("author deliberately wrote this".into())
    );
    assert_eq!(table.rows[0][1].value, CellValue::Text("1 + 1 = 3".into()));
}
