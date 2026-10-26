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
                layout: None,
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
    assert_ne!(captured.regions[0].id, first);
    assert_eq!(captured.regions[0].cell, Some(first));
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
        .find(|r| r.cell.unwrap_or(r.id) == second)
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
    assert_eq!(document.version, 13);
    document.version = 11;
    assert!(matches!(document.validate(), Err(Error::Unsupported)));
    let mut copy = Store::memory().unwrap();
    let ids = copy.import_native(&bytes, &access).unwrap();
    let copied = copy.blocks(ids[0], 0, 10, &access).unwrap().remove(0);
    let Content::Table(table) = &copied.content else {
        panic!()
    };
    assert_ne!(table.rows[0][0].id, first);
    assert_eq!(copied.regions[0].cell, Some(table.rows[0][0].id));
    assert_ne!(copied.regions[0].id, table.rows[0][0].id);
    assert_ne!(copied.regions[0].strokes, block.regions[0].strokes);
    assert!(copy
        .region_snapshot(copied.id, table.rows[0][0].id, &access)
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
    assert_eq!(restored.schema_version().unwrap(), 21);
    assert_eq!(
        restored.source_ink(block.id, &access).unwrap(),
        store.source_ink(block.id, &access).unwrap()
    );
    let plain = crypto::decrypt(&backup.bytes, crypto::Credential::Password(password)).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    assert_eq!(json["schema"], 21);
    json["schema"] = 19.into();
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
    plain[8..12].copy_from_slice(&19u32.to_be_bytes());
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
fn local_cell_targets_are_independent_across_tables_and_fresh_protection_still_applies() {
    use stem_core::security::Scope;
    let (mut store, access, note, block, first, _) = fixture();
    let duplicate = store
        .append_content(note, block.content.clone(), &access)
        .unwrap();
    let _captured = capture(&mut store, &access, &block, first, 0);
    let second = store
        .append_cell_capture(Id::new(), duplicate.id, 0, first, vec![stroke(1)], &access)
        .unwrap();
    let original = store.block(block.id, &access).unwrap();
    assert_ne!(original.regions[0].id, second.regions[0].id);
    assert_eq!(original.regions[0].cell, second.regions[0].cell);
    assert_ne!(
        store
            .region_snapshot(block.id, first, &access)
            .unwrap()
            .strokes()[0]
            .id,
        store
            .region_snapshot(duplicate.id, first, &access)
            .unwrap()
            .strokes()[0]
            .id
    );
    let scoped = common::access([Scope::Block(duplicate.id)]);
    let locked = store
        .set_block_lock(duplicate.id, second.revision, true, &scoped)
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
    assert_eq!(store.source_ink(duplicate.id, &scoped).unwrap().len(), 1);
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
                    layout: None,
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
            entity: Some(note),
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
    assert_eq!(doc.blocks[0].current.regions[0].cell, Some(cell));
    assert_ne!(doc.blocks[0].current.regions[0].id, cell);
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

fn native_bytes(store: &Store, note: Id, access: &Access) -> Vec<u8> {
    use stem_core::portable::ExportProtection;
    store
        .finish_native_export(
            store.prepare_native_export(&[note], true, access).unwrap(),
            ExportProtection::Plaintext {
                acknowledge_protected: true,
            },
            &mut TestAuth(true),
            true,
        )
        .unwrap()
        .bytes
}
#[test]
fn repeated_local_cell_keys_have_independent_proposals_and_atomic_batch_acceptance() {
    let (mut store, access, note, first, cell, _) = fixture();
    let second = store
        .append_content(note, first.content.clone(), &access)
        .unwrap();
    let first = capture(&mut store, &access, &first, cell, 0);
    let second = capture(&mut store, &access, &second, cell, 1);
    let a = propose(&mut store, &access, first.id, cell);
    let b = propose(&mut store, &access, second.id, cell);
    store
        .correct_proposal(
            a.id,
            Content::Text("first deliberate error".into()),
            &access,
        )
        .unwrap();
    store
        .correct_proposal(b.id, Content::Numeric("001".into()), &access)
        .unwrap();
    let accepted = store
        .accept_proposals(&[(a.id, 0), (b.id, 0)], true, &access)
        .unwrap();
    assert_eq!(accepted.len(), 2);
    let Content::Table(a) = &store.block(first.id, &access).unwrap().content else {
        panic!()
    };
    let Content::Table(b) = &store.block(second.id, &access).unwrap().content else {
        panic!()
    };
    assert_eq!(
        a.rows[0][0].value,
        CellValue::Text("first deliberate error".into())
    );
    assert_eq!(b.rows[0][0].value, CellValue::Numeric("001".into()));
}
#[test]
fn local_cell_keys_may_equal_existing_entity_and_block_identifiers() {
    let (mut store, access, note, _, _, _) = fixture();
    let content = Content::Table(Table {
        rows: vec![vec![Cell {
            id: note,
            value: CellValue::Text("local".into()),
        }]],
        header_rows: 0,
        layout: None,
    });
    let block = store.append_content(note, content, &access).unwrap();
    let block = capture(&mut store, &access, &block, note, 0);
    assert_ne!(block.regions[0].id, note);
    assert_eq!(block.regions[0].cell, Some(note));
    let second = store
        .append_content(
            note,
            Content::Table(Table {
                rows: vec![vec![Cell {
                    id: Id::new(),
                    value: CellValue::Text("local".into()),
                }]],
                header_rows: 0,
                layout: None,
            }),
            &access,
        )
        .unwrap();
    let replaced = store
        .edit_content(
            second.id,
            0,
            Content::Table(Table {
                rows: vec![vec![Cell {
                    id: second.id,
                    value: CellValue::Text("local".into()),
                }]],
                header_rows: 0,
                layout: None,
            }),
            &access,
        )
        .unwrap();
    let captured = capture(&mut store, &access, &replaced, second.id, 1);
    assert_ne!(captured.regions[0].id, second.id);
    let mut copy = Store::memory().unwrap();
    let roots = copy
        .import_native(&native_bytes(&store, note, &access), &access)
        .unwrap();
    assert!(copy.blocks(roots[0], 0, 10, &access).is_ok());
}
#[test]
fn version_eleven_aliases_remain_readable_and_mix_with_new_independent_regions() {
    use stem_core::portable::NativeDocument;
    let (mut store, access, note, block, first, _) = fixture();
    capture(&mut store, &access, &block, first, 0);
    let mut legacy = NativeDocument::decode(&native_bytes(&store, note, &access)).unwrap();
    for block in &mut legacy.blocks {
        for state in std::iter::once(&mut block.current)
            .chain(block.history.iter_mut().map(|history| &mut history.block))
        {
            for region in &mut state.regions {
                if let Some(cell) = region.cell.take() {
                    region.id = cell;
                }
            }
        }
    }
    legacy.version = 11;
    legacy.validate().unwrap();
    use sha2::{Digest, Sha256};
    let payload = serde_json::to_string(&legacy).unwrap();
    let checksum = format!("{:x}", Sha256::digest(payload.as_bytes()));
    let bytes =
        serde_json::to_vec(&serde_json::json!({"sha256":checksum,"payload":payload})).unwrap();
    let mut restored = Store::memory().unwrap();
    let roots = restored.import_native(&bytes, &access).unwrap();
    let copied = restored.blocks(roots[0], 0, 10, &access).unwrap().remove(0);
    let Content::Table(table) = &copied.content else {
        panic!()
    };
    let first = table.rows[0][0].id;
    let second = table.rows[0][1].id;
    assert!(copied.regions[0].cell.is_none());
    assert_eq!(copied.regions[0].id, first);
    let proposal = propose(&mut restored, &access, copied.id, first);
    let accepted = restored.accept_proposal(proposal.id, 0, &access).unwrap();
    let mixed = capture(&mut restored, &access, &accepted, second, 1);
    assert!(mixed
        .regions
        .iter()
        .any(|region| region.id == first && region.cell.is_none()));
    assert!(mixed
        .regions
        .iter()
        .any(|region| region.cell == Some(second) && region.id != second));
}

#[test]
fn schema19_legacy_shape_preserves_pending_review_and_raw_receipt_during_migration() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("legacy.db");
    let access = Access::public();
    let mut store = Store::open(&path).unwrap();
    let note = store
        .create(Kind::Note, None, "legacy shape", &access)
        .unwrap();
    let ticket = Id::new();
    let cell = Id::new();
    let table = store
        .append_content(
            note.id,
            Content::Table(Table {
                rows: vec![vec![Cell {
                    id: cell,
                    value: CellValue::Text("legacy".into()),
                }]],
                header_rows: 0,
                layout: None,
            }),
            &access,
        )
        .unwrap();
    store
        .append_cell_capture(ticket, table.id, 0, cell, vec![stroke(0)], &access)
        .unwrap();
    drop(store);
    // Construct the documented schema19 representation, including retained rows.
    fn old_payload(encoded: &str) -> String {
        let mut block: Block = serde_json::from_str(encoded).unwrap();
        for region in &mut block.regions {
            if let Some(cell) = region.cell.take() {
                region.id = cell;
            }
        }
        serde_json::to_string(&block).unwrap()
    }
    let db = rusqlite::Connection::open(&path).unwrap();
    for name in ["blocks", "history"] {
        let rows = db
            .prepare(&format!("SELECT rowid,payload FROM {name}"))
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        for (row, payload) in rows {
            db.execute(
                &format!("UPDATE {name} SET payload=?1 WHERE rowid=?2"),
                rusqlite::params![old_payload(&payload), row],
            )
            .unwrap();
        }
    }
    let rows=db.prepare("SELECT operation,item,before_row,after_row FROM edit_changes WHERE table_name='blocks'").unwrap().query_map([],|row|Ok((row.get::<_,i64>(0)?,row.get::<_,String>(1)?,row.get::<_,Option<String>>(2)?,row.get::<_,Option<String>>(3)?))).unwrap().collect::<std::result::Result<Vec<_>,_>>().unwrap();
    for (operation, item, before, after) in rows {
        let convert = |encoded: Option<String>| {
            encoded.map(|encoded| {
                let mut fields: Vec<serde_json::Value> = serde_json::from_str(&encoded).unwrap();
                let at = fields.len() - 1;
                fields[at] = old_payload(fields[at].as_str().unwrap()).into();
                serde_json::to_string(&fields).unwrap()
            })
        };
        db.execute("UPDATE edit_changes SET before_row=?1,after_row=?2 WHERE operation=?3 AND table_name='blocks' AND item=?4",rusqlite::params![convert(before),convert(after),operation,item]).unwrap();
    }
    drop(db);
    let mut store = Store::open(&path).unwrap();
    let pending = propose(&mut store, &access, table.id, cell);
    drop(store);
    let db = rusqlite::Connection::open(&path).unwrap();
    let old_block: String = db
        .query_row(
            "SELECT payload FROM blocks WHERE id=?1",
            [table.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let receipt: String = db
        .query_row(
            "SELECT fingerprint FROM capture_receipts WHERE ticket=?1",
            [ticket.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!old_block.contains("\"cell\":"));
    db.pragma_update(None, "user_version", 19).unwrap();
    drop(db);
    let mut migrated = Store::open(&path).unwrap();
    assert_eq!(migrated.schema_version().unwrap(), 21);
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT payload FROM blocks WHERE id=?1",
            [table.id.to_string()],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        old_block
    );
    assert_eq!(
        db.query_row(
            "SELECT fingerprint FROM capture_receipts WHERE ticket=?1",
            [ticket.to_string()],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        receipt
    );
    drop(db);
    assert_eq!(
        migrated.proposal(pending.id, &access).unwrap().fingerprint,
        pending.fingerprint
    );
    assert!(migrated
        .capture_committed(ticket, note.id, &access)
        .unwrap());
    let accepted = migrated.accept_proposal(pending.id, 0, &access).unwrap();
    assert!(accepted.regions[0].cell.is_none());
    assert_eq!(migrated.source_ink(table.id, &access).unwrap().len(), 1);
}

#[test]
fn raw_cell_bookmark_copies_its_local_selector_separately_from_global_region_identity() {
    use stem_core::{
        portable::NativeDocument,
        store::{BookmarkState, Camera, CaretPoint, CaretTarget, SelectionBookmark, ViewSettings},
    };
    let (mut store, access, note, block, cell, _) = fixture();
    let captured = capture(&mut store, &access, &block, cell, 0);
    let point = CaretPoint {
        block: block.id,
        revision: captured.revision,
        region: Some(cell),
        target: CaretTarget::Ink {
            stroke: captured.source_ids[0],
            sample: 0,
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
    let mut copied = Store::memory().unwrap();
    let roots = copied
        .import_native(&native_bytes(&store, note, &access), &access)
        .unwrap();
    let doc = NativeDocument::decode(&native_bytes(&copied, roots[0], &access)).unwrap();
    let selected = &doc.attachments.views[0].bookmark.as_ref().unwrap().anchor;
    assert_eq!(selected.region, doc.blocks[0].current.regions[0].cell);
    assert_ne!(selected.region, Some(doc.blocks[0].current.regions[0].id));
    assert_eq!(
        copied
            .restore_view(doc.attachments.views[0].id, &access)
            .unwrap()
            .bookmark_state,
        BookmarkState::Current
    );
}
#[test]
fn duplicate_local_targets_within_one_table_and_cell_bindings_on_prose_are_invalid() {
    let (mut store, access, _, block, cell, _) = fixture();
    let block = capture(&mut store, &access, &block, cell, 0);
    let mut duplicate = block.clone();
    let mut extra = duplicate.regions[0].clone();
    extra.id = Id::new();
    let source = Id::new();
    extra.strokes = vec![source];
    duplicate.source_ids.push(source);
    duplicate.visible_strokes.push(source);
    duplicate.regions.push(extra);
    assert!(matches!(
        duplicate.validate(),
        Err(Error::Invalid("duplicate table ink target"))
    ));
    let mut prose = block;
    prose.content = Content::Ink;
    assert!(prose.validate().is_err());
}

#[test]
fn version_ten_tables_with_repeated_local_ids_import_and_capture_without_rewriting_values() {
    use sha2::{Digest, Sha256};
    use stem_core::portable::NativeDocument;
    let (mut store, access, note, first, _, _) = fixture();
    store
        .append_content(note, first.content.clone(), &access)
        .unwrap();
    let mut old = NativeDocument::decode(&native_bytes(&store, note, &access)).unwrap();
    old.version = 10;
    old.validate().unwrap();
    let payload = serde_json::to_string(&old).unwrap();
    let sha256 = format!("{:x}", Sha256::digest(payload.as_bytes()));
    let bytes =
        serde_json::to_vec(&serde_json::json!({"sha256":sha256,"payload":payload})).unwrap();
    let mut imported = Store::memory().unwrap();
    let roots = imported.import_native(&bytes, &access).unwrap();
    let blocks = imported.blocks(roots[0], 0, 10, &access).unwrap();
    assert_eq!(blocks.len(), 2);
    let Content::Table(table) = &blocks[0].content else {
        panic!()
    };
    let cell = table.rows[0][0].id;
    let one = capture(&mut imported, &access, &blocks[0], cell, 0);
    let two = capture(&mut imported, &access, &blocks[1], cell, 1);
    assert_ne!(one.regions[0].id, two.regions[0].id);
    assert_eq!(one.regions[0].cell, two.regions[0].cell);
    assert_eq!(one.content, blocks[0].content);
    assert_eq!(two.content, blocks[1].content);
}

#[test]
fn typed_table_structure_is_atomic_revision_bound_and_undoable() {
    use stem_core::objects::TableEdit;
    let (mut store, access, note, block, first, second) = fixture();
    let inserted = store
        .edit_table(
            block.id,
            block.revision,
            TableEdit::InsertRow { index: 0 },
            &access,
        )
        .unwrap();
    let Content::Table(table) = &inserted.content else {
        panic!()
    };
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[1][0].id, first);
    assert_eq!(table.rows[1][1].id, second);
    assert!(table.rows[0]
        .iter()
        .all(|cell| cell.value == CellValue::Text(String::new())));
    assert!(matches!(
        store.edit_table(
            block.id,
            block.revision,
            TableEdit::RemoveRow { index: 0 },
            &access
        ),
        Err(Error::Conflict)
    ));
    store.undo(note, &access).unwrap();
    assert_eq!(
        store.block(block.id, &access).unwrap().content,
        block.content
    );
    store.redo(note, &access).unwrap();
    assert_eq!(
        store.block(block.id, &access).unwrap().content,
        inserted.content
    );
    let current = store.block(block.id, &access).unwrap();
    assert!(store
        .edit_table(
            block.id,
            current.revision,
            TableEdit::RemoveColumn { index: 7 },
            &access
        )
        .is_err());
    assert_eq!(store.block(block.id, &access).unwrap(), current);
}

#[test]
fn table_structure_preserves_headers_and_rejects_empty_or_oversized_candidates() {
    use stem_core::objects::TableEdit;
    let (_, _, _, block, first, _) = fixture();
    let Content::Table(mut table) = block.content else {
        panic!()
    };
    table.header_rows = 1;
    table
        .restructure(TableEdit::InsertRow { index: 0 })
        .unwrap();
    assert_eq!(table.header_rows, 2);
    table
        .restructure(TableEdit::InsertRow { index: 2 })
        .unwrap();
    assert_eq!(table.header_rows, 2);
    table
        .restructure(TableEdit::RemoveRow { index: 0 })
        .unwrap();
    assert_eq!(table.header_rows, 1);
    assert_eq!(table.rows[0][0].id, first);
    table
        .restructure(TableEdit::InsertColumn { index: 1 })
        .unwrap();
    assert_eq!(table.rows[0][0].id, first);
    table
        .restructure(TableEdit::RemoveColumn { index: 1 })
        .unwrap();
    table
        .restructure(TableEdit::RemoveColumn { index: 1 })
        .unwrap();
    let before = table.clone();
    assert!(table
        .restructure(TableEdit::RemoveColumn { index: 0 })
        .is_err());
    assert_eq!(table, before);
    table.rows = (0..1000)
        .map(|_| {
            vec![Cell {
                id: Id::new(),
                value: CellValue::Text(String::new()),
            }]
        })
        .collect();
    let before = table.clone();
    assert!(table
        .restructure(TableEdit::InsertRow { index: 1000 })
        .is_err());
    assert_eq!(table, before);
}

#[test]
fn structural_table_edits_do_not_guess_handwriting_placement() {
    use stem_core::objects::TableEdit;
    let (mut store, access, _, block, first, _) = fixture();
    let captured = capture(&mut store, &access, &block, first, 0);
    assert!(matches!(
        store.edit_table(
            block.id,
            captured.revision,
            TableEdit::InsertRow { index: 0 },
            &access
        ),
        Err(Error::ReviewRequired)
    ));
    assert_eq!(store.block(block.id, &access).unwrap(), captured);
}

#[test]
fn structural_table_failure_rolls_back_and_fresh_lock_denies_edits() {
    use stem_core::{objects::TableEdit, security::Scope};
    let (mut store, access, _, block, _, _) = fixture();
    store.inject_failure(FailurePoint::BeforeCommit);
    assert!(matches!(
        store.edit_table(
            block.id,
            block.revision,
            TableEdit::InsertColumn { index: 0 },
            &access
        ),
        Err(Error::Interrupted)
    ));
    assert_eq!(store.block(block.id, &access).unwrap(), block);
    let scoped = common::access([Scope::Block(block.id)]);
    let locked = store
        .set_block_lock(block.id, block.revision, true, &scoped)
        .unwrap();
    assert!(matches!(
        store.edit_table(
            block.id,
            locked.revision,
            TableEdit::RemoveColumn { index: 0 },
            &access
        ),
        Err(Error::Denied)
    ));
    assert_eq!(store.block(block.id, &scoped).unwrap(), locked);
}

#[test]
fn host_table_structure_command_runs_through_the_writer() {
    use stem_core::{
        host::{Command, Host, Request},
        objects::TableEdit,
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let (block, note) = {
        let mut store = Store::open(&path).unwrap();
        let access = Access::public();
        let note = store
            .create(Kind::Note, None, "structure", &access)
            .unwrap();
        let block = store
            .append_content(
                note.id,
                Content::Table(Table {
                    rows: vec![vec![Cell {
                        id: Id::new(),
                        value: CellValue::Text("retained".into()),
                    }]],
                    header_rows: 0,
                    layout: None,
                }),
                &access,
            )
            .unwrap();
        (block, note.id)
    };
    let host = Host::open(path, 8).unwrap();
    let request = serde_json::to_string(&Request {
        version: 1,
        command: Command::EditTable {
            block: block.id,
            revision: block.revision,
            edit: TableEdit::InsertColumn { index: 1 },
        },
    })
    .unwrap();
    let response = host.request(&request).unwrap();
    assert!(response.ok, "{:?}", response.error);
    let edited: Block = serde_json::from_value(response.data.unwrap()).unwrap();
    assert_eq!(edited.entity, note);
    let Content::Table(table) = edited.content else {
        panic!()
    };
    assert_eq!(table.rows[0].len(), 2);
    assert_eq!(table.rows[0][0].value, CellValue::Text("retained".into()));
    assert!(!host.request(&request).unwrap().ok);
    let transfer = serde_json::to_string(&Request {
        version: 1,
        command: Command::TransferCell {
            source: stem_core::store::CellTarget {
                block: edited.id,
                revision: edited.revision,
                cell: table.rows[0][0].id,
            },
            target: stem_core::store::CellTarget {
                block: edited.id,
                revision: edited.revision,
                cell: table.rows[0][1].id,
            },
            mode: stem_core::store::CellTransferMode::Copy,
        },
    })
    .unwrap();
    let response = host.request(&transfer).unwrap();
    assert!(response.ok, "{:?}", response.error);
    let result: Vec<Block> = serde_json::from_value(response.data.unwrap()).unwrap();
    let Content::Table(copied) = &result[0].content else {
        panic!()
    };
    assert_eq!(copied.rows[0][1].value, CellValue::Text("retained".into()));
    assert!(!host.request(&transfer).unwrap().ok);
}

fn cell_layout() -> stem_core::objects::TableLayout {
    stem_core::objects::TableLayout {
        coordinate_space: "synthetic-local-units".into(),
        origin: stem_core::objects::Position { x: 0., y: 0. },
        row_heights: vec![20.],
        column_widths: vec![40., 40.],
        ink_origins: vec![],
    }
}

#[test]
fn cell_layout_reflows_ink_without_rewriting_observations_and_undo_restores_placement() {
    use stem_core::objects::TableEdit;
    let (mut store, access, note, block, _, second) = fixture();
    let block = store
        .set_table_layout(block.id, block.revision, cell_layout(), &access)
        .unwrap();
    let mut original = stroke(0);
    original.points[0].x = 45.;
    original.points[1].x = 46.;
    let captured = store
        .append_cell_capture(
            Id::new(),
            block.id,
            block.revision,
            second,
            vec![original.clone()],
            &access,
        )
        .unwrap();
    let moved = store
        .edit_table(
            block.id,
            captured.revision,
            TableEdit::InsertSizedColumn {
                index: 0,
                width: 10.,
            },
            &access,
        )
        .unwrap();
    let scene = store.table_ink_scene(block.id, &access).unwrap();
    assert_eq!(scene.len(), 1);
    assert_eq!(scene[0].stroke, original);
    assert_eq!(scene[0].source_to_surface.tx, 10.);
    assert_eq!(
        store.source_ink(block.id, &access).unwrap(),
        vec![original.clone()]
    );
    let snapshot = store.region_snapshot(block.id, second, &access).unwrap();
    assert_eq!(snapshot.strokes()[0].points[0].x, 5.);
    store.undo(note, &access).unwrap();
    assert_eq!(
        store.table_ink_scene(block.id, &access).unwrap()[0]
            .source_to_surface
            .tx,
        0.
    );
    store.redo(note, &access).unwrap();
    assert_eq!(
        store.table_ink_scene(block.id, &access).unwrap()[0]
            .source_to_surface
            .tx,
        10.
    );
    let current = store.block(block.id, &access).unwrap();
    let resized = store
        .edit_table(
            block.id,
            current.revision,
            TableEdit::ResizeColumn {
                index: 1,
                width: 70.,
            },
            &access,
        )
        .unwrap();
    assert!(resized.revision > moved.revision);
    let scene = store.table_ink_scene(block.id, &access).unwrap();
    assert_eq!(scene[0].source_to_surface.tx, 40.);
    assert_eq!(scene[0].source_to_surface.a, 1.);
    assert_eq!(scene[0].stroke, original);
}

#[test]
fn captures_before_and_after_reflow_align_in_cell_local_recognition_vectors() {
    use stem_core::objects::TableEdit;
    let (mut store, access, _, block, _, second) = fixture();
    let block = store
        .set_table_layout(block.id, 0, cell_layout(), &access)
        .unwrap();
    let mut first = stroke(0);
    first.points[0].x = 45.;
    let block = store
        .append_cell_capture(
            Id::new(),
            block.id,
            block.revision,
            second,
            vec![first.clone()],
            &access,
        )
        .unwrap();
    let block = store
        .edit_table(
            block.id,
            block.revision,
            TableEdit::ResizeColumn {
                index: 0,
                width: 80.,
            },
            &access,
        )
        .unwrap();
    let mut second_stroke = stroke(0);
    second_stroke.points[0].x = 85.;
    let block = store
        .append_cell_capture(
            Id::new(),
            block.id,
            block.revision,
            second,
            vec![second_stroke.clone()],
            &access,
        )
        .unwrap();
    let snapshot = store.region_snapshot(block.id, second, &access).unwrap();
    assert_eq!(snapshot.strokes()[0].points[0].x, 5.);
    assert_eq!(snapshot.strokes()[1].points[0].x, 5.);
    assert_eq!(
        store.source_ink(block.id, &access).unwrap(),
        vec![first, second_stroke]
    );
    let mut wrong = stroke(0);
    wrong.coordinate_space = "unmapped-device-space".into();
    assert!(store
        .append_cell_capture(
            Id::new(),
            block.id,
            block.revision,
            second,
            vec![wrong],
            &access
        )
        .is_err());
    assert_eq!(store.block(block.id, &access).unwrap(), block);
}

#[test]
fn removing_an_ink_cell_hides_its_vectors_but_retains_originals_and_undo() {
    use stem_core::objects::TableEdit;
    let (mut store, access, note, block, first, _) = fixture();
    let block = store
        .set_table_layout(block.id, 0, cell_layout(), &access)
        .unwrap();
    let block = capture(&mut store, &access, &block, first, 0);
    let original = store.source_ink(block.id, &access).unwrap();
    let deleted = store
        .edit_table(
            block.id,
            block.revision,
            TableEdit::RemoveColumn { index: 0 },
            &access,
        )
        .unwrap();
    assert!(deleted.regions.is_empty());
    assert!(deleted.visible_strokes.is_empty());
    assert_eq!(deleted.source_ids, block.source_ids);
    assert!(store.table_ink_scene(block.id, &access).unwrap().is_empty());
    assert_eq!(store.source_ink(block.id, &access).unwrap(), original);
    store.undo(note, &access).unwrap();
    assert_eq!(
        store.table_ink_scene(block.id, &access).unwrap()[0].stroke,
        original[0]
    );
}

#[test]
fn adopting_legacy_cell_ink_requires_complete_explicit_origins_and_matching_space() {
    use stem_core::objects::{CellInkOrigin, Position};
    let (mut store, access, _, block, first, _) = fixture();
    let block = capture(&mut store, &access, &block, first, 0);
    assert!(store
        .set_table_layout(block.id, block.revision, cell_layout(), &access)
        .is_err());
    let mut layout = cell_layout();
    layout.ink_origins.push(CellInkOrigin {
        cell: first,
        stroke: block.source_ids[0],
        origin: Position { x: -10., y: 2. },
    });
    layout.coordinate_space = "unknown".into();
    assert!(store
        .set_table_layout(block.id, block.revision, layout.clone(), &access)
        .is_err());
    layout.coordinate_space = "synthetic-local-units".into();
    let adopted = store
        .set_table_layout(block.id, block.revision, layout.clone(), &access)
        .unwrap();
    layout.ink_origins[0].origin.x = 0.;
    assert!(matches!(
        store.set_table_layout(block.id, adopted.revision, layout, &access),
        Err(Error::Conflict)
    ));
    assert_eq!(
        store.table_ink_scene(block.id, &access).unwrap()[0]
            .source_to_surface
            .tx,
        10.
    );
}

#[test]
fn native_layout_copy_remaps_origins_and_rejects_pre13_even_when_only_in_history() {
    use stem_core::portable::{ExportProtection, NativeDocument};
    let (mut store, access, note, block, first, _) = fixture();
    let block = store
        .set_table_layout(block.id, 0, cell_layout(), &access)
        .unwrap();
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
    let mut doc = NativeDocument::decode(&bytes).unwrap();
    assert_eq!(doc.version, 13);
    doc.version = 12;
    assert!(matches!(doc.validate(), Err(Error::Unsupported)));
    doc.blocks[0].current.content = Content::Table(Table {
        rows: vec![vec![Cell {
            id: Id::new(),
            value: CellValue::Text(String::new()),
        }]],
        header_rows: 0,
        layout: None,
    });
    assert!(matches!(doc.validate(), Err(Error::Unsupported)));
    let imported = store.import_native(&bytes, &access).unwrap();
    let copied = store.blocks(imported[0], 0, 10, &access).unwrap().remove(0);
    let scene = store.table_ink_scene(copied.id, &access).unwrap();
    assert_ne!(scene[0].stroke.id, block.source_ids[0]);
    assert_ne!(scene[0].cell, first);
    assert_eq!(scene[0].source_to_surface.tx, 0.);
}

#[test]
fn layout_classic_backup_preserves_origins_and_refuses_schema20_history() {
    use stem_core::{backup::BackupOptions, crypto};
    let (mut store, access, note, block, first, _) = fixture();
    let block = store
        .set_table_layout(block.id, block.revision, cell_layout(), &access)
        .unwrap();
    let block = capture(&mut store, &access, &block, first, 0);
    let proposal = propose(&mut store, &access, block.id, first);
    store.accept_proposal(proposal.id, 0, &access).unwrap();
    store.undo(note, &access).unwrap();
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
    assert_eq!(restored.schema_version().unwrap(), 21);
    assert_eq!(
        restored.source_ink(block.id, &access).unwrap(),
        store.source_ink(block.id, &access).unwrap()
    );
    let plain = crypto::decrypt(&backup.bytes, crypto::Credential::Password(password)).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    assert_eq!(json["schema"], 21);
    json["schema"] = 20.into();
    let disguised = crypto::encrypt(&serde_json::to_vec(&json).unwrap(), password).unwrap();
    assert!(matches!(
        Store::restore_backup_password(&disguised.bytes, password),
        Err(Error::Unsupported)
    ));
}

#[test]
fn layout_stream_backup_preserves_origins_and_refuses_schema20() {
    use stem_core::{backup::BackupOptions, crypto};
    let (mut store, access, _, block, first, _) = fixture();
    let block = store
        .set_table_layout(block.id, block.revision, cell_layout(), &access)
        .unwrap();
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
    plain[8..12].copy_from_slice(&20u32.to_be_bytes());
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
fn layout_invalid_geometry_and_stale_cell_proposals_leave_authored_source_intact() {
    use stem_core::objects::TableEdit;
    let (mut store, access, _, block, first, _) = fixture();
    let mut invalid = cell_layout();
    invalid.column_widths[0] = 0.;
    assert!(store
        .set_table_layout(block.id, 0, invalid, &access)
        .is_err());
    let mut invalid = cell_layout();
    invalid.row_heights.clear();
    assert!(store
        .set_table_layout(block.id, 0, invalid, &access)
        .is_err());
    assert_eq!(store.block(block.id, &access).unwrap(), block);
    let block = store
        .set_table_layout(block.id, 0, cell_layout(), &access)
        .unwrap();
    let block = capture(&mut store, &access, &block, first, 0);
    let proposal = propose(&mut store, &access, block.id, first);
    let changed = store
        .edit_table(
            block.id,
            block.revision,
            TableEdit::ResizeRow {
                index: 0,
                height: 30.,
            },
            &access,
        )
        .unwrap();
    assert!(matches!(
        store.accept_proposal(proposal.id, 0, &access),
        Err(Error::Conflict)
    ));
    assert!(store
        .edit_table(
            block.id,
            changed.revision,
            TableEdit::InsertSizedRow {
                index: 1,
                height: f64::INFINITY
            },
            &access
        )
        .is_err());
    assert_eq!(store.block(block.id, &access).unwrap(), changed);
    let scoped = common::access([stem_core::security::Scope::Block(block.id)]);
    store
        .set_block_lock(block.id, changed.revision, true, &scoped)
        .unwrap();
    assert!(matches!(
        store.table_ink_scene(block.id, &access),
        Err(Error::Denied)
    ));
}

fn cell_target(block: &Block, cell: Id) -> stem_core::store::CellTarget {
    stem_core::store::CellTarget {
        block: block.id,
        revision: block.revision,
        cell,
    }
}
#[test]
fn typed_cell_cut_copy_preserves_grid_identity_literals_and_undo() {
    use stem_core::store::CellTransferMode::{Copy, Move};
    let (mut store, access, note, block, first, second) = fixture();
    let copied = store
        .transfer_cell(
            cell_target(&block, second),
            cell_target(&block, first),
            Copy,
            &access,
        )
        .unwrap()
        .remove(0);
    let Content::Table(t) = &copied.content else {
        panic!()
    };
    assert_eq!(t.rows[0][0].id, first);
    assert_eq!(t.rows[0][1].id, second);
    assert_eq!(t.rows[0][0].value, CellValue::Numeric("001".into()));
    assert_eq!(t.rows[0][0].value, t.rows[0][1].value);
    store.undo(note, &access).unwrap();
    assert_eq!(
        store.block(block.id, &access).unwrap().content,
        block.content
    );
    let current = store.block(block.id, &access).unwrap();
    let moved = store
        .transfer_cell(
            cell_target(&current, second),
            cell_target(&current, first),
            Move,
            &access,
        )
        .unwrap()
        .remove(0);
    let Content::Table(t) = &moved.content else {
        panic!()
    };
    assert_eq!(t.rows[0][1].value, CellValue::Text(String::new()));
    assert_eq!(t.rows[0][0].value, CellValue::Numeric("001".into()));
    store.undo(note, &access).unwrap();
    assert_eq!(
        store.block(block.id, &access).unwrap().content,
        block.content
    );
    store.redo(note, &access).unwrap();
    assert_eq!(
        store.block(block.id, &access).unwrap().content,
        moved.content
    );
}
#[test]
fn same_table_ink_cut_retargets_existing_sources_and_keeps_replaced_ink_for_undo() {
    use stem_core::store::CellTransferMode::Move;
    let (mut store, access, note, block, first, second) = fixture();
    let block = store
        .set_table_layout(block.id, 0, cell_layout(), &access)
        .unwrap();
    let block = capture(&mut store, &access, &block, first, 0);
    let block = capture(&mut store, &access, &block, second, 0);
    let originals = store.source_ink(block.id, &access).unwrap();
    let moved = store
        .transfer_cell(
            cell_target(&block, first),
            cell_target(&block, second),
            Move,
            &access,
        )
        .unwrap()
        .remove(0);
    assert_eq!(moved.source_ids, block.source_ids);
    assert_eq!(moved.regions.len(), 1);
    assert_eq!(moved.regions[0].cell, Some(second));
    assert_eq!(moved.visible_strokes, vec![originals[0].id]);
    let scene = store.table_ink_scene(block.id, &access).unwrap();
    assert_eq!(scene[0].stroke, originals[0]);
    assert_eq!(scene[0].source_to_surface.tx, 40.);
    assert_eq!(store.source_ink(block.id, &access).unwrap(), originals);
    store.undo(note, &access).unwrap();
    let undone = store.block(block.id, &access).unwrap();
    assert_eq!(undone.regions, block.regions);
    assert_eq!(undone.content, block.content);
    assert_eq!(undone.visible_strokes, block.visible_strokes);
}
#[test]
fn same_table_ink_copy_creates_independent_identity_without_fabricating_contact_samples() {
    use stem_core::store::CellTransferMode::Copy;
    let (mut store, access, note, block, first, second) = fixture();
    let block = store
        .set_table_layout(block.id, 0, cell_layout(), &access)
        .unwrap();
    let block = capture(&mut store, &access, &block, first, 0);
    let original = store.source_ink(block.id, &access).unwrap().remove(0);
    let copied = store
        .transfer_cell(
            cell_target(&block, first),
            cell_target(&block, second),
            Copy,
            &access,
        )
        .unwrap()
        .remove(0);
    assert_eq!(copied.regions.len(), 2);
    assert_ne!(copied.regions[0].id, copied.regions[1].id);
    let scene = store.table_ink_scene(block.id, &access).unwrap();
    assert_eq!(scene.len(), 2);
    let mut independent = scene[1].stroke.clone();
    assert_ne!(independent.id, original.id);
    independent.id = original.id;
    assert_eq!(independent, original);
    assert_eq!(scene[1].source_to_surface.tx, 40.);
    assert_eq!(scene[0].source_to_surface.tx, 0.);
    store.undo(note, &access).unwrap();
    assert_eq!(store.table_ink_scene(block.id, &access).unwrap().len(), 1);
    assert_eq!(store.source_ink(block.id, &access).unwrap().len(), 2);
    store.redo(note, &access).unwrap();
    assert_eq!(store.table_ink_scene(block.id, &access).unwrap().len(), 2);
}
#[test]
fn between_table_cut_is_one_atomic_undo_and_failed_commit_leaves_both_unchanged() {
    use stem_core::store::CellTransferMode::Move;
    let (mut store, access, note, source, first, second) = fixture();
    let source = store
        .set_table_layout(source.id, 0, cell_layout(), &access)
        .unwrap();
    let source = capture(&mut store, &access, &source, first, 0);
    let target = store.append_content(note, source.content.clone(), &access);
    // A content-only append cannot smuggle source placements into a new block.
    assert!(target.is_err());
    let Content::Table(mut content) = source.content.clone() else {
        panic!()
    };
    content.layout.as_mut().unwrap().ink_origins.clear();
    let target = store
        .append_content(note, Content::Table(content), &access)
        .unwrap();
    store.inject_failure(FailurePoint::BeforeCommit);
    assert!(matches!(
        store.transfer_cell(
            cell_target(&source, first),
            cell_target(&target, second),
            Move,
            &access
        ),
        Err(Error::Interrupted)
    ));
    assert_eq!(store.block(source.id, &access).unwrap(), source);
    assert_eq!(store.block(target.id, &access).unwrap(), target);
    let result = store
        .transfer_cell(
            cell_target(&source, first),
            cell_target(&target, second),
            Move,
            &access,
        )
        .unwrap();
    assert!(result[0].visible_strokes.is_empty());
    assert_eq!(store.table_ink_scene(target.id, &access).unwrap().len(), 1);
    store.undo(note, &access).unwrap();
    assert_eq!(
        store.block(source.id, &access).unwrap().content,
        source.content
    );
    assert_eq!(
        store.block(target.id, &access).unwrap().content,
        target.content
    );
    assert_eq!(store.table_ink_scene(source.id, &access).unwrap().len(), 1);
    assert!(store
        .table_ink_scene(target.id, &access)
        .unwrap()
        .is_empty());
    store.redo(note, &access).unwrap();
    assert_eq!(store.table_ink_scene(target.id, &access).unwrap().len(), 1);
}
#[test]
fn cell_transfer_rechecks_revisions_and_scope() {
    use stem_core::{security::Scope, store::CellTransferMode::Copy};
    let (mut store, access, _, block, first, second) = fixture();
    let block = store
        .set_table_layout(block.id, 0, cell_layout(), &access)
        .unwrap();
    let block = capture(&mut store, &access, &block, first, 0);
    let mut stale = cell_target(&block, second);
    stale.revision -= 1;
    assert!(matches!(
        store.transfer_cell(cell_target(&block, first), stale, Copy, &access),
        Err(Error::Conflict)
    ));
    let scoped = common::access([Scope::Block(block.id)]);
    let locked = store
        .set_block_lock(block.id, block.revision, true, &scoped)
        .unwrap();
    assert!(matches!(
        store.transfer_cell(
            cell_target(&locked, first),
            cell_target(&locked, second),
            Copy,
            &access
        ),
        Err(Error::Denied)
    ));
    assert_eq!(store.source_ink(block.id, &scoped).unwrap().len(), 1);
}

#[test]
fn cross_master_cell_copy_is_destination_undoable_and_foreign_space_needs_explicit_mapping() {
    use stem_core::store::CellTransferMode::{Copy, Move};
    let (mut store, access, _, source, first, second) = fixture();
    let source = store
        .set_table_layout(source.id, 0, cell_layout(), &access)
        .unwrap();
    let source = capture(&mut store, &access, &source, first, 0);
    let note = store
        .create(Kind::Note, None, "destination", &access)
        .unwrap();
    let Content::Table(mut content) = source.content.clone() else {
        panic!()
    };
    let layout = content.layout.as_mut().unwrap();
    layout.ink_origins.clear();
    layout.coordinate_space = "different-unmapped-units".into();
    let target = store
        .append_content(note.id, Content::Table(content), &access)
        .unwrap();
    assert!(matches!(
        store.transfer_cell(
            cell_target(&source, first),
            cell_target(&target, second),
            Copy,
            &access
        ),
        Err(Error::Invalid("cell transfer coordinate space"))
    ));
    assert_eq!(store.block(target.id, &access).unwrap(), target);
    let target = store
        .set_table_layout(
            target.id,
            target.revision,
            {
                let mut layout = cell_layout();
                layout.coordinate_space = "different-unmapped-units".into();
                layout
            },
            &access,
        )
        .unwrap();
    // Coordinate identity is immutable once adopted: use a separate compatible table.
    let Content::Table(mut content) = source.content.clone() else {
        panic!()
    };
    content.layout.as_mut().unwrap().ink_origins.clear();
    let compatible = store
        .append_content(note.id, Content::Table(content), &access)
        .unwrap();
    let result = store
        .transfer_cell(
            cell_target(&source, first),
            cell_target(&compatible, second),
            Copy,
            &access,
        )
        .unwrap();
    assert_eq!(result[0], source);
    assert_eq!(store.block(source.id, &access).unwrap(), source);
    let scene = store.table_ink_scene(compatible.id, &access).unwrap();
    assert_eq!(scene[0].source_to_surface.tx, 40.);
    assert!(matches!(
        store.transfer_cell(
            cell_target(&source, first),
            cell_target(&result[1], second),
            Move,
            &access
        ),
        Err(Error::Invalid("cell move master surface"))
    ));
    store.undo(note.id, &access).unwrap();
    assert!(store
        .table_ink_scene(compatible.id, &access)
        .unwrap()
        .is_empty());
    assert_eq!(store.block(source.id, &access).unwrap(), source);
    assert_eq!(store.block(target.id, &access).unwrap(), target);
}
#[test]
fn copied_cell_does_not_resurrect_erased_contacts_and_unplaced_legacy_ink_refuses_transfer() {
    use stem_core::store::CellTransferMode::Copy;
    let (mut store, access, _, block, first, second) = fixture();
    let block = capture(&mut store, &access, &block, first, 0);
    assert!(matches!(
        store.transfer_cell(
            cell_target(&block, first),
            cell_target(&block, second),
            Copy,
            &access
        ),
        Err(Error::ReviewRequired)
    ));
    let mut layout = cell_layout();
    layout.ink_origins.push(stem_core::objects::CellInkOrigin {
        cell: first,
        stroke: block.source_ids[0],
        origin: stem_core::objects::Position { x: 0., y: 0. },
    });
    let block = store
        .set_table_layout(block.id, block.revision, layout, &access)
        .unwrap();
    let block = store
        .erase_strokes(block.id, block.revision, &block.visible_strokes, &access)
        .unwrap();
    let copied = store
        .transfer_cell(
            cell_target(&block, first),
            cell_target(&block, second),
            Copy,
            &access,
        )
        .unwrap()
        .remove(0);
    assert!(copied.visible_strokes.is_empty());
    assert_eq!(copied.regions.len(), 2);
    assert_eq!(store.source_ink(block.id, &access).unwrap().len(), 2);
    assert!(store.table_ink_scene(block.id, &access).unwrap().is_empty());
}

#[test]
fn copied_contact_provenance_and_placement_survive_native_copy_and_encrypted_restore() {
    use stem_core::{
        backup::BackupOptions, portable::ExportProtection, store::CellTransferMode::Copy,
    };
    let (mut store, access, note, block, first, second) = fixture();
    let block = store
        .set_table_layout(block.id, 0, cell_layout(), &access)
        .unwrap();
    let block = capture(&mut store, &access, &block, first, 0);
    let block = capture(&mut store, &access, &block, second, 0);
    let block = store
        .transfer_cell(
            cell_target(&block, first),
            cell_target(&block, second),
            Copy,
            &access,
        )
        .unwrap()
        .remove(0);
    let expected = store.table_ink_scene(block.id, &access).unwrap();
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
    let mut independent = Store::memory().unwrap();
    let entities = independent.import_native(&bytes, &access).unwrap();
    let imported = independent
        .blocks(entities[0], 0, 10, &access)
        .unwrap()
        .remove(0);
    let scene = independent.table_ink_scene(imported.id, &access).unwrap();
    assert_eq!(scene.len(), 2);
    for (copied, original) in scene.iter().zip(&expected) {
        assert_ne!(copied.stroke.id, original.stroke.id);
        assert_eq!(
            copied.stroke.capture_session,
            original.stroke.capture_session
        );
        assert_eq!(copied.stroke.points, original.stroke.points);
        assert_eq!(copied.source_to_surface, original.source_to_surface);
    }
    assert_eq!(
        independent.source_ink(imported.id, &access).unwrap().len(),
        3
    );
    let password = "Synthetic copied-contact provenance backup password";
    let backup = store
        .finish_backup(
            store
                .prepare_backup(
                    BackupOptions {
                        include_original_ink: true,
                        include_authorised_source_assets: false,
                    },
                    &access,
                )
                .unwrap(),
            password,
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    let restored = Store::restore_backup_password(&backup.bytes, password).unwrap();
    let scene = restored.table_ink_scene(block.id, &access).unwrap();
    for (restored, original) in scene.iter().zip(&expected) {
        assert_eq!(restored.stroke, original.stroke);
        assert_eq!(restored.source_to_surface, original.source_to_surface);
    }
    assert_eq!(restored.source_ink(block.id, &access).unwrap().len(), 3);
}

#[test]
fn corrupted_cell_origins_refuse_transfer_without_panicking_or_inserting_source_rows() {
    use stem_core::{
        objects::{CellInkOrigin, Position},
        store::CellTransferMode::Copy,
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("corrupt-cell.db");
    let mut store = Store::open(&path).unwrap();
    let access = Access::public();
    let note = store
        .create(Kind::Note, None, "corruption", &access)
        .unwrap();
    let first = Id::new();
    let second = Id::new();
    let block = store
        .append_content(
            note.id,
            Content::Table(Table {
                rows: vec![vec![
                    Cell {
                        id: first,
                        value: CellValue::Text("source".into()),
                    },
                    Cell {
                        id: second,
                        value: CellValue::Text("destination".into()),
                    },
                ]],
                header_rows: 0,
                layout: Some(cell_layout()),
            }),
            &access,
        )
        .unwrap();
    let block = capture(&mut store, &access, &block, first, 0);
    let mut corrupt = block.clone();
    let Content::Table(table) = &mut corrupt.content else {
        panic!()
    };
    table
        .layout
        .as_mut()
        .unwrap()
        .ink_origins
        .push(CellInkOrigin {
            cell: first,
            stroke: Id::new(),
            origin: Position { x: 0., y: 0. },
        });
    let db = rusqlite::Connection::open(&path).unwrap();
    let payload = serde_json::to_string(&corrupt).unwrap();
    db.execute(
        "UPDATE blocks SET payload=?1 WHERE id=?2",
        rusqlite::params![payload, block.id.to_string()],
    )
    .unwrap();
    assert!(matches!(
        store.transfer_cell(
            cell_target(&block, first),
            cell_target(&block, second),
            Copy,
            &access
        ),
        Err(Error::Invalid(_))
    ));
    let actual: String = db
        .query_row(
            "SELECT payload FROM blocks WHERE id=?1",
            [block.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(actual, payload);
    let count: i64 = db
        .query_row("SELECT count(*) FROM strokes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}
