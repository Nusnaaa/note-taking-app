mod common;
use common::*;
use serde_json::{json, Value};
use stem_core::{
    content::Content,
    host::{Command, Host, PdfCommand, Request, Response},
    security::Scope,
    sources::PdfView,
    store::Kind,
    Id,
};
fn call(h: &Host, command: Command) -> Response {
    h.request(
        &serde_json::to_string(&Request {
            version: 1,
            command,
        })
        .unwrap(),
    )
    .unwrap()
}
fn ok(h: &Host, command: Command) -> Value {
    let r = call(h, command);
    assert!(r.ok, "{:?}", r.error);
    r.data.unwrap()
}
fn pdf(h: &Host, command: PdfCommand) -> Value {
    ok(h, Command::Pdf { command })
}
fn denied(h: &Host, command: PdfCommand) {
    let r = call(h, Command::Pdf { command });
    assert!(!r.ok);
    assert!(r.data.is_none());
    assert!(r.generation.is_none());
}
fn id(v: &Value, field: &str) -> Id {
    Id::parse(v[field].as_str().unwrap()).unwrap()
}
fn setup(h: &Host) -> (Id, Id, String) {
    let note = ok(
        h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "synthetic context".into(),
        },
    );
    let context = id(&note, "id");
    let revision = "a".repeat(64);
    let source = pdf(
        h,
        PdfCommand::Link {
            context,
            locator: "content://synthetic/pdf".into(),
            revision: revision.clone(),
        },
    );
    (context, id(&source, "id"), revision)
}
#[test]
fn live_read_lease_survives_pen_and_view_changes_but_not_release_or_restart() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("read.db");
    let h = Host::open(path.clone(), 8).unwrap();
    let (context, source, revision) = setup(&h);
    let read = pdf(&h, PdfCommand::PrepareRead { source });
    let lease = id(&read, "lease");
    assert!((1..=300_000).contains(&read["valid_for_ms"].as_u64().unwrap()));
    assert_eq!(read["revision"], revision);
    assert_eq!(read["locator"], "content://synthetic/pdf");
    let observed = stroke(0);
    let captured = ok(
        &h,
        Command::Capture {
            ticket: Id::new(),
            entity: context,
            strokes: vec![observed.clone()],
        },
    );
    pdf(
        &h,
        PdfCommand::ValidateRead {
            lease,
            observed_revision: revision.clone(),
        },
    );
    pdf(
        &h,
        PdfCommand::SetView {
            source,
            view: PdfView {
                page: 42,
                zoom: 2.,
                ..Default::default()
            },
        },
    );
    pdf(
        &h,
        PdfCommand::ValidateRead {
            lease,
            observed_revision: revision.clone(),
        },
    );
    let ink = ok(
        &h,
        Command::SourceInk {
            block: id(&captured, "id"),
        },
    );
    assert_eq!(ink, json!([observed]));
    pdf(&h, PdfCommand::ReleaseRead { lease });
    denied(
        &h,
        PdfCommand::ValidateRead {
            lease,
            observed_revision: revision.clone(),
        },
    );
    let lease = id(&pdf(&h, PdfCommand::PrepareRead { source }), "lease");
    drop(h);
    let h = Host::open(path, 8).unwrap();
    denied(
        &h,
        PdfCommand::ValidateRead {
            lease,
            observed_revision: revision,
        },
    );
    assert_eq!(pdf(&h, PdfCommand::Source { source })["view"]["page"], 42);
}
#[test]
fn revocation_and_missing_sources_retire_reads_without_touching_authored_work() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("revoke.db"), 8).unwrap();
    let (context, source, revision) = setup(&h);
    let block = ok(
        &h,
        Command::Append {
            entity: context,
            content: Content::Text("independent authored observation".into()),
        },
    );
    let lease = id(&pdf(&h, PdfCommand::PrepareRead { source }), "lease");
    pdf(&h, PdfCommand::MarkMissing { source });
    denied(
        &h,
        PdfCommand::ValidateRead {
            lease,
            observed_revision: revision,
        },
    );
    denied(&h, PdfCommand::PrepareRead { source });
    pdf(&h, PdfCommand::Revoke { source });
    denied(&h, PdfCommand::PrepareRead { source });
    assert_eq!(
        ok(
            &h,
            Command::Block {
                block: id(&block, "id")
            }
        ),
        block
    );
    assert_eq!(
        pdf(&h, PdfCommand::Source { source })["availability"],
        "Revoked"
    );
}
#[test]
fn relock_scope_changes_and_failed_authentication_retire_old_capabilities() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("lock.db"), 8).unwrap();
    let (context, source, revision) = setup(&h);
    let lease = id(&pdf(&h, PdfCommand::PrepareRead { source }), "lease");
    ok(&h, Command::Relock {});
    denied(
        &h,
        PdfCommand::ValidateRead {
            lease,
            observed_revision: revision.clone(),
        },
    );
    let lease = id(&pdf(&h, PdfCommand::PrepareRead { source }), "lease");
    assert!(h
        .authenticate(
            [Scope::Entity(context)].into_iter().collect(),
            Box::new(TestAuth(false))
        )
        .is_err());
    denied(
        &h,
        PdfCommand::ValidateRead {
            lease,
            observed_revision: revision.clone(),
        },
    );
    h.authenticate(
        [Scope::Entity(context)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    let lease = id(&pdf(&h, PdfCommand::PrepareRead { source }), "lease");
    ok(
        &h,
        Command::SetLock {
            entity: context,
            revision: 0,
            locked: true,
        },
    );
    denied(
        &h,
        PdfCommand::ValidateRead {
            lease,
            observed_revision: revision,
        },
    );
    ok(&h, Command::Relock {});
    denied(&h, PdfCommand::Source { source });
    denied(&h, PdfCommand::PrepareRead { source });
}
#[test]
fn read_and_revision_tokens_are_bounded_private_and_revision_checked() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("bounded.db"), 8).unwrap();
    let (context, source, revision) = setup(&h);
    let mut leases = Vec::new();
    for _ in 0..32 {
        leases.push(id(&pdf(&h, PdfCommand::PrepareRead { source }), "lease"));
    }
    denied(&h, PdfCommand::PrepareRead { source });
    pdf(&h, PdfCommand::ReleaseRead { lease: leases[0] });
    let replacement = id(&pdf(&h, PdfCommand::PrepareRead { source }), "lease");
    denied(
        &h,
        PdfCommand::ValidateRead {
            lease: replacement,
            observed_revision: "b".repeat(64),
        },
    );
    denied(
        &h,
        PdfCommand::ValidateRead {
            lease: replacement,
            observed_revision: revision,
        },
    );
    denied(
        &h,
        PdfCommand::ReplaceRevision {
            review: Id::new(),
            confirmed: true,
        },
    );
    let review = pdf(
        &h,
        PdfCommand::ReviewRevision {
            source,
            locator: "content://synthetic/next".into(),
            revision: "b".repeat(64),
        },
    );
    ok(
        &h,
        Command::Append {
            entity: context,
            content: Content::Text("new work after revision review".into()),
        },
    );
    denied(
        &h,
        PdfCommand::ReplaceRevision {
            review: id(&review, "review"),
            confirmed: true,
        },
    );
    denied(
        &h,
        PdfCommand::ReplaceRevision {
            review: id(&review, "review"),
            confirmed: true,
        },
    );
    for _ in 0..32 {
        pdf(
            &h,
            PdfCommand::ReviewRevision {
                source,
                locator: "content://synthetic/next".into(),
                revision: "b".repeat(64),
            },
        );
    }
    denied(
        &h,
        PdfCommand::ReviewRevision {
            source,
            locator: "content://synthetic/next".into(),
            revision: "b".repeat(64),
        },
    );
}
#[test]
fn reviewed_pdf_replacement_updates_lineage_and_retires_the_old_live_read() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("revision.db"), 8).unwrap();
    let (context, source, revision) = setup(&h);
    let block = ok(
        &h,
        Command::Append {
            entity: context,
            content: Content::Text("source observation".into()),
        },
    );
    let anchor = pdf(
        &h,
        PdfCommand::CreateAnchor {
            source,
            block: id(&block, "id"),
            page: 2,
            bounds: stem_core::objects::Rect {
                x: 0.1,
                y: 0.2,
                width: 0.3,
                height: 0.4,
            },
            pinned: false,
        },
    );
    let lease = id(&pdf(&h, PdfCommand::PrepareRead { source }), "lease");
    let review = pdf(
        &h,
        PdfCommand::ReviewRevision {
            source,
            locator: "content://synthetic/next".into(),
            revision: "b".repeat(64),
        },
    );
    assert_eq!(review["affected_anchors"], 1);
    pdf(
        &h,
        PdfCommand::ReplaceRevision {
            review: id(&review, "review"),
            confirmed: true,
        },
    );
    denied(
        &h,
        PdfCommand::ValidateRead {
            lease,
            observed_revision: revision.clone(),
        },
    );
    assert_eq!(
        pdf(&h, PdfCommand::Source { source })["previous_revisions"],
        json!([revision])
    );
    assert_eq!(
        pdf(
            &h,
            PdfCommand::Anchor {
                anchor: id(&anchor, "id")
            }
        )["health"],
        "NeedsReview"
    );
    pdf(
        &h,
        PdfCommand::ReconnectAnchor {
            anchor: id(&anchor, "id"),
            page: 3,
            bounds: stem_core::objects::Rect {
                x: 0.,
                y: 0.,
                width: 0.5,
                height: 0.5,
            },
            confirmed: true,
        },
    );
    assert_eq!(
        pdf(
            &h,
            PdfCommand::Anchor {
                anchor: id(&anchor, "id")
            }
        )["health"],
        "Current"
    );
}
#[test]
fn rejecting_a_prepared_revision_consumes_it_without_changing_the_source() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("reject.db"), 8).unwrap();
    let (_, source, revision) = setup(&h);
    let prepared = pdf(
        &h,
        PdfCommand::ReviewRevision {
            source,
            locator: "content://synthetic/replacement".into(),
            revision: "b".repeat(64),
        },
    );
    let review = id(&prepared, "review");
    denied(
        &h,
        PdfCommand::ReplaceRevision {
            review,
            confirmed: false,
        },
    );
    denied(
        &h,
        PdfCommand::ReplaceRevision {
            review,
            confirmed: true,
        },
    );
    assert_eq!(pdf(&h, PdfCommand::Source { source })["revision"], revision);
}
#[test]
fn ordinary_json_cannot_supply_a_review_plan_or_platform_grant() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("json.db"), 8).unwrap();
    assert!(h.request(r#"{"version":1,"command":{"op":"Pdf","command":{"op":"Reacquire","new_platform_grant":true}}}"#).is_err());
    assert!(h.request(r#"{"version":1,"command":{"op":"Pdf","command":{"op":"ReplaceRevision","review":{"source":"private","generation":"1"},"confirmed":true}}}"#).is_err());
}

struct Grant(bool);
impl stem_core::host::PdfGrantVerifier for Grant {
    fn verify(
        &mut self,
        _: Id,
        _: &stem_core::sources::PdfSelectionTarget,
        locator: &str,
        revision: &str,
    ) -> bool {
        assert!(locator.starts_with("content://"));
        assert_eq!(revision.len(), 64);
        self.0
    }
}
fn selection(h: &Host, source: Id, revision: &str) -> Value {
    h.prepare_pdf_selection(
        stem_core::sources::PdfSelectionTarget::Relink { source },
        "content://synthetic/picked".into(),
        revision.into(),
        Box::new(Grant(true)),
    )
    .unwrap()
}
#[test]
fn trusted_selection_is_reviewed_and_same_revision_regrant_preserves_source_identity() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("selected.db"), 8).unwrap();
    let (context, source, revision) = setup(&h);
    pdf(&h, PdfCommand::Revoke { source });
    let prepared = selection(&h, source, &revision);
    assert_eq!(prepared["source"], json!(source));
    assert_eq!(prepared["changed_revision"], false);
    assert_eq!(
        pdf(&h, PdfCommand::Source { source })["availability"],
        "Revoked"
    );
    let restored = h
        .finish_pdf_selection(id(&prepared, "review"), true, Box::new(Grant(true)))
        .unwrap();
    assert_eq!(restored["id"], json!(source));
    assert_eq!(restored["context"], json!(context));
    assert_eq!(restored["availability"], "Available");
    assert_eq!(restored["previous_revisions"], json!([]));
    assert!(h
        .finish_pdf_selection(id(&prepared, "review"), true, Box::new(Grant(true)))
        .is_err());
}
#[test]
fn changed_revision_can_be_reacquired_from_revoked_state_without_silent_anchor_relocation() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("edition.db"), 8).unwrap();
    let (context, source, revision) = setup(&h);
    let block = ok(
        &h,
        Command::Append {
            entity: context,
            content: Content::Text("original observation".into()),
        },
    );
    let bounds = stem_core::objects::Rect {
        x: 0.,
        y: 0.,
        width: 0.5,
        height: 0.5,
    };
    let anchor = pdf(
        &h,
        PdfCommand::CreateAnchor {
            source,
            block: id(&block, "id"),
            page: 3,
            bounds,
            pinned: false,
        },
    );
    pdf(&h, PdfCommand::Revoke { source });
    let prepared = selection(&h, source, &"b".repeat(64));
    assert_eq!(prepared["changed_revision"], true);
    assert_eq!(prepared["affected_anchors"], 1);
    h.finish_pdf_selection(id(&prepared, "review"), true, Box::new(Grant(true)))
        .unwrap();
    let changed = pdf(&h, PdfCommand::Source { source });
    assert_eq!(changed["previous_revisions"], json!([revision]));
    assert_eq!(changed["revision"], "b".repeat(64));
    let retained = pdf(
        &h,
        PdfCommand::Anchor {
            anchor: id(&anchor, "id"),
        },
    );
    assert_eq!(retained["page"], 3);
    assert_eq!(retained["health"], "NeedsReview");
    assert_eq!(
        ok(
            &h,
            Command::Block {
                block: id(&block, "id")
            }
        ),
        block
    );
}
#[test]
fn selection_requires_fresh_verification_on_acceptance_and_rejection_is_single_use() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("fresh.db"), 8).unwrap();
    let (_, source, revision) = setup(&h);
    pdf(&h, PdfCommand::Revoke { source });
    let denied = h.prepare_pdf_selection(
        stem_core::sources::PdfSelectionTarget::Relink { source },
        "content://synthetic/picked".into(),
        revision.clone(),
        Box::new(Grant(false)),
    );
    assert!(denied.is_err());
    let prepared = selection(&h, source, &revision);
    assert!(h
        .finish_pdf_selection(id(&prepared, "review"), true, Box::new(Grant(false)))
        .is_err());
    assert!(h
        .finish_pdf_selection(id(&prepared, "review"), true, Box::new(Grant(true)))
        .is_err());
    assert_eq!(
        pdf(&h, PdfCommand::Source { source })["availability"],
        "Revoked"
    );
    let prepared = selection(&h, source, &revision);
    assert!(h
        .finish_pdf_selection(id(&prepared, "review"), false, Box::new(Grant(true)))
        .is_err());
    assert!(h
        .finish_pdf_selection(id(&prepared, "review"), true, Box::new(Grant(true)))
        .is_err());
}
#[test]
fn selection_reviews_are_stale_safe_scope_checked_and_cleared_on_relock() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("stale-selection.db"), 8).unwrap();
    let (context, source, revision) = setup(&h);
    let prepared = selection(&h, source, &revision);
    ok(
        &h,
        Command::Append {
            entity: context,
            content: Content::Text("new work".into()),
        },
    );
    assert!(h
        .finish_pdf_selection(id(&prepared, "review"), true, Box::new(Grant(true)))
        .is_err());
    let prepared = selection(&h, source, &revision);
    ok(&h, Command::Relock {});
    assert!(h
        .finish_pdf_selection(id(&prepared, "review"), true, Box::new(Grant(true)))
        .is_err());
    h.authenticate(
        [Scope::Entity(context)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    let rev = ok(&h, Command::Read { id: context })["revision"]
        .as_u64()
        .unwrap();
    ok(
        &h,
        Command::SetLock {
            entity: context,
            revision: rev,
            locked: true,
        },
    );
    ok(&h, Command::Relock {});
    assert!(h
        .prepare_pdf_selection(
            stem_core::sources::PdfSelectionTarget::Relink { source },
            "content://synthetic/picked".into(),
            revision,
            Box::new(Grant(true))
        )
        .is_err());
}
#[test]
fn new_link_selection_does_not_create_source_until_explicit_acceptance_and_uses_preallocated_identity(
) {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("new-selection.db"), 8).unwrap();
    let n = ok(
        &h,
        Command::Create {
            kind: Kind::Note,
            parent: None,
            title: "owner".into(),
        },
    );
    let review = h
        .prepare_pdf_selection(
            stem_core::sources::PdfSelectionTarget::Link {
                context: id(&n, "id"),
            },
            "content://synthetic/selected".into(),
            "a".repeat(64),
            Box::new(Grant(true)),
        )
        .unwrap();
    let source = id(&review, "source");
    denied(&h, PdfCommand::Source { source });
    let accepted = h
        .finish_pdf_selection(id(&review, "review"), true, Box::new(Grant(true)))
        .unwrap();
    assert_eq!(accepted["id"], json!(source));
    assert_eq!(accepted["acquisition"], "ExternalLink");
}
#[test]
fn bounded_selection_plans_can_be_released_and_cannot_be_supplied_as_json_grants() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::open(d.path().join("bound-selection.db"), 8).unwrap();
    let (_, source, revision) = setup(&h);
    let mut plans = Vec::new();
    for _ in 0..32 {
        plans.push(selection(&h, source, &revision));
    }
    assert!(h
        .prepare_pdf_selection(
            stem_core::sources::PdfSelectionTarget::Relink { source },
            "content://synthetic/picked".into(),
            revision.clone(),
            Box::new(Grant(true))
        )
        .is_err());
    pdf(
        &h,
        PdfCommand::ReleaseSelection {
            review: id(&plans[0], "review"),
        },
    );
    selection(&h, source, &revision);
    assert!(h.request(r#"{"version":1,"command":{"op":"Pdf","command":{"op":"FinishSelection","review":"00000000-0000-0000-0000-000000000000","confirmed":true,"new_platform_grant":true}}}"#).is_err());
}
