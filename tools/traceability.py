#!/usr/bin/env python3
"""Preserve source assertions and generate the current, deliberately partial map.

Fixture grouping reduces duplicate setup, not the number of obligations. A source
candidate remains NOT RUN until its entire expected outcome is actually exercised.
Related executable tests are evidence for subsets, never an automatic PASS.
"""
import argparse
import csv
import hashlib
import io
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "docs/imported/05_SOURCES/verification"
OUT = ROOT / "docs/verification"

# Explicit mapping reviewed against the approved prose, not inferred from ID counts.
# (requirements, implementation, related executable test modules, remaining work)
GROUPS = {
    'capture': ('NB-CORE-001 NB-PEN-001 NB-PEN-003 NB-PEN-005 NB-DATA-002', 'ink.rs capture.rs host.rs host/ownership.rs store/writing.rs store/layers.rs store/capture_recovery.rs store/objects.rs store/positions.rs store/authoring.rs store/workspace.rs store.rs android/backend/src/main/java/org/stemnotebook/backend/LibraryOwnership.kt android/backend/src/main/java/org/stemnotebook/backend/ActiveCaptureJournal.kt android/backend/src/main/java/org/stemnotebook/backend/JournaledCaptureSession.kt android/backend/src/main/java/org/stemnotebook/backend/BackendWriter.kt android/backend/src/main/java/org/stemnotebook/backend/NotebookBackend.kt', 'writing recovery capture capture_privacy capture_recovery layer_capture stroke_style authoring objects positions workspace host android:ActiveCaptureJournalTest android:JournaledCaptureIntegrationTest android:BackendWriterTest android:BackendAdmissionIntegrationTest android:MotionCaptureTest unit:host/ownership.rs:tests:sqlite_managed_ownership_survives_commits_and_failed_duplicate_connections android:BackendOwnershipTest', 'Observed Android capture, durable outbox/replay, single writer, retry receipts and reserve release exist. Native authoring/query admission has separate count/encoded-byte budgets and authoring priority. Durable active-prefix journaling and scoped review/recovery exist. Android reserved authoring/query count/input-byte admission, queued-read yielding and coalesced status are integrated. Explicit Android finger opt-in, immutable contact provenance, stylus preemption and v1-v4 journal compatibility are covered. Reviewed same-canvas sealed-draft retargeting checks fresh authority, checksum/layer revision and absence of native receipts, preserving observations and honest commit status. Explicit discard retires caller-held frames across interrupted sessions before unlink without retaining session observations. Full optional-work/resource scheduling, all-operation retirement and device coverage remain open.'),
    'editable': ('NB-CORE-002 NB-CORE-005 NB-REC-002', 'content.rs objects.rs converters.rs render.rs store/writing.rs store/regions.rs store/objects.rs', 'writing converters objects regions render', 'Editable technical objects, selected-stroke regions and vector maths PDF exist. Component-cell proposals, full scene rendering and actual local recogniser integration remain open.'),
    'offline': ('NB-CORE-004 NB-AI-001 NB-AI-002', 'processing.rs sources.rs security.rs host.rs', 'recognition sources host', 'Android library removes INTERNET and isolates its PDF parser. Final application/process enforcement, parser client integration and device egress tests remain open; LLM tools are M2.'),
    'source-ink': ('NB-PEN-002 NB-REC-003 NB-DATA-003', 'ink.rs capture.rs store/writing.rs portable.rs backup.rs android/backend/src/main/java/org/stemnotebook/backend/InkAccumulator.kt android/backend/src/main/java/org/stemnotebook/backend/ObservedInk.kt android/backend/src/main/java/org/stemnotebook/backend/CaptureOutbox.kt', 'writing interrupted_capture stroke_style android:ActiveCaptureJournalTest android:JournaledCaptureIntegrationTest android:InkAccumulatorTest android:MotionCaptureTest android:InterruptedCaptureTest', 'Exact original samples survive edit/history expiry; stable active prefixes and explicit completed/interrupted provenance preserve source hashes/receipts through schema-10/native-v5 termination upgrades and schema-11/native-v6 bookmark upgrades and schema-13/native-v7 chosen-style upgrades. Library-bound binary active journaling, lifecycle sealing and scoped recovery review preserve observed prefixes. Whole-note presentation and shared source anchors remain open.'),
    'modes': ('NB-PEN-004 NB-REC-010', 'store/organisation.rs store/exercises.rs portable.rs', 'writing exercises', 'Linked exercises follow the master; independent copies snapshot and may override modes. Persisted host controls and broader mode workflows remain open.'),
    'recognition': ('NB-REC-001 NB-REC-004 NB-REC-005 NB-REC-006 NB-REC-007', 'processing.rs store/proposals.rs store/regions.rs', 'recognition engines regions processing_work', 'Revision-safe block, selected batch and region proposals and region split/merge exist. Stored proposal SQL owner/key binding and current permission checks precede private payload parsing and reject misbound corrections. Component-cell recognition, actual recogniser integration and clipboard host actions remain open.'),
    'model-lifecycle': ('NB-REC-012 NB-SUP-003', 'processing.rs provisioning.rs store/engines.rs capture.rs', 'recognition engines capture backup provisioning processing_work', 'Durable resumable local staging, recogniser/converter probe/rollback and resource leases exist. Guarded worker reader clones retain deletion reservations until the final descriptor retires, including nested clones and thread transfer. Owned recognition/conversion work pins verified adapters, links lifecycle cancellation epochs and keeps resources until real worker retirement. Bounded host/JNI job transport, actual recogniser, complete lease cleanup, hard worker isolation and Android scheduling remain open.'),
    'organisation': ('NB-DOC-001 NB-ORG-001 NB-ORG-003 NB-ORG-004 NB-ORG-014', 'store/organisation.rs host.rs host/deletion.rs', 'organisation deletion host_deletion unit:host/deletion.rs:tests:expired_and_capacity_bounded_reviews_never_delete_content android:BackendAdmissionIntegrationTest android:JournaledCaptureIntegrationTest android:DeletionCaptureIntegrationTest', 'Core I0/I1 parent, identity, move and backlink rules tested; anchored navigation, full-object moves and scale coverage remain open. M1 Journal rules deferred.'),
    'exercises': ('NB-DOC-003 NB-DOC-004 NB-DATA-008', 'host.rs store/exercises.rs store/navigation.rs store/positions.rs store/authoring.rs store/workspace.rs store/writing.rs store/objects.rs store.rs portable.rs attachments.rs', 'exercises exercise_migrations objects positions workspace backup host android:BackendAdmissionIntegrationTest', 'Shared identity/status, protections, reassignment undo, independent copies, flat search/navigation and persisted view anchors exist. Native coalesced content-free admission subscriptions, SQLite-managed exclusive Host ownership and Android ownership through full resource retirement exist; Android coalesced admission subscriptions exist. Revision-bound view caret/selection bookmarks restore with current/stale state and fresh authority. Fresh-authority workspace/pane/tool activation, redacted placeholders and purge retirement exist. Complete authoring configuration/pane workflows and operation history remain open.'),
    'rich-objects': ('NB-DOC-002 NB-CAN-001 NB-CAN-002 NB-CAN-003 NB-CAN-004', 'content.rs objects.rs store/objects.rs store/layers.rs store/editing.rs store/writing.rs attachments.rs export.rs render.rs', 'objects layers export backup editing render android:BackendAdmissionIntegrationTest', 'Technical data, image assets, groups/layouts/cameras and durable authoring undo exist. Canvas block layers, independent embed visibility, unresolved references and byte-bounded paginated projections are integrated. Layer-aware capture routing, custom nested-embed assignment, full rendering, flow/reflow, overlays and follow-object behaviour remain open.'),
    'pdf-sources': ('NB-REF-001 NB-REF-002 NB-REF-003 NB-PDF-001 NB-PDF-002', 'sources.rs backup.rs security.rs host.rs host/pdf.rs android/backend/src/main/java/org/stemnotebook/backend/PdfProvider.kt android/backend/src/main/java/org/stemnotebook/backend/PdfWorkerService.kt android/backend/src/main/java/org/stemnotebook/backend/PdfConnection.kt android/backend/src/main/java/org/stemnotebook/backend/PdfPixels.kt android/backend/src/main/java/org/stemnotebook/backend/PdfParserClient.kt android/backend/src/main/java/org/stemnotebook/backend/PdfWorkspace.kt android/backend/src/main/java/org/stemnotebook/backend/PdfSelectionManager.kt android/backend/src/main/java/org/stemnotebook/backend/NotebookBackend.kt android/backend/src/main/java/org/stemnotebook/backend/NativeBridge.java bridge/src/lib.rs', 'sources pdf_revisions backup host_pdf unit:sources.rs:selection_tests:selected_revision_transaction_rolls_back_and_preserves_independent_evidence unit:sources.rs:selection_tests:retained_pdf_commit_failure_does_not_publish_asset_or_change_acquisition android:PdfSelectionTest android:PdfRetentionTest android:PdfProviderTest android:PdfWorkerCleanupTest android:PdfPixelsTest android:PdfParserClientTest android:PdfWorkspaceTest android:PdfConnectionTest', 'Native bounded opaque read/review capabilities, source/view/anchor workflows, isolated Android binding/death/timeout control, bounded raw-pixel IPC and hash-frozen disposable read caches are integrated. Current access is checked before/after parsing; relock/source changes and native expiry retire frames. Trusted off-writer picker hashing, fresh native callback preparation/acceptance, stable-ID same/changed-revision regrant and atomic source/anchor review are implemented. Explicit bounded native retained-asset staging and managed/historical rendering via checked shared read leases, exact chunk hashes and independent authority/provenance are implemented. Typed Android retention follows explicit link acceptance with bounded frozen bytes, final permission/policy checks and commit receipts. Comprehensive permission/lifecycle/navigation cases and actual service/device execution remain open.'),
    'export': ('NB-CORE-003 NB-EXP-001 NB-EXP-002 NB-EXP-003 NB-EXP-004 NB-DATA-006 NB-DATA-007', 'portable.rs attachments.rs store/layers.rs store/authoring.rs backup.rs crypto.rs export.rs converters.rs render.rs', 'portable converters exercises objects positions workspace authoring stroke_style layers layer_capture backup export render regions android:BackendAdmissionIntegrationTest', 'Native v9 per-view authoring/canvas layers/chosen styles/bookmarks/interrupted provenance/regions/rich attachments, schema-8/9/10/11/12/13/14/15 encrypted whole restore and paginated vector maths/selectable text PDF exist. Static embeds, clipped images/canvas fidelity, scalable backup, complete lineage and PDF-linked native portability remain open.'),
    'recovery': ('NB-DATA-001 NB-DATA-004 NB-DATA-005', 'store.rs store/writing.rs store/editing.rs store/organisation.rs portable.rs backup.rs capture.rs host.rs host/deletion.rs', 'deletion host_deletion android:DeletionCaptureIntegrationTest recovery writing portable exercises exercise_migrations workspace capture backup editing host', 'Transactional authoring undo, capture retry receipts, durable Android outbox and schema-8/9/10/11/12/13/14/15 separate-library restore are tested. Native content-free coalesced admission status and draining close exist. All-operation history, chunked backup, remaining interruption paths and resource integration remain open. Android coalesced admission status is integrated.'),
    'security': ('NB-SEC-001 NB-SEC-002 NB-SEC-003', 'security.rs host.rs store.rs store/writing.rs store/exercises.rs store/objects.rs portable.rs backup.rs sources.rs export.rs', 'capture_privacy organisation writing recognition portable sources exercises objects backup export host editing android:BackendAdmissionIntegrationTest', 'Scoped core checks, trusted JNI authentication callback, platform proof and Android manifest exclusions exist. Final app/device enforcement, complete process integration and privacy coverage remain open.'),
    'search': ('NB-SUP-001', 'store/writing.rs store/navigation.rs content.rs', 'writing exercises', 'Literal accepted-content/title search and flat exercise projections exist. Complete field/heading/label paths and scale need acceptance; no raw-ink/equivalence search claim.'),
    'help': ('NB-SUP-002', 'help.rs', 'converters', 'Bundled recovery topics are tested; contextual host integration and full recovery workflows remain open.'),
    'governance': ('NB-OPS-001 NB-OPS-003 NB-OPS-004 NB-OPS-005', '', 'recovery host', 'Approved Rust/SQLite/Android strategy recorded. Rust/JNI/Android automated checks now run; complete implementation and whole-requirement acceptance remain NOT MET; no iOS gate.'),
    'post-hi': ('NB-UX-001 NB-REC-008 NB-OPS-002', '', '', 'Production H.I. follows backend acceptance. Owner-private S Pen, real recogniser quality and converter usability testing waits for the runnable app.'),
}

# Candidates in each family share setup, but their full assertion text is retained.
# Multi-outcome candidates are deliberately NOT collapsed into a passing unit test.
CANDIDATES = {
    "capture": "TC-01 TC-02 SV-12 S4C-018 ST5E-T-01",
    "editable": "TC-03 ST5E-T-06 ST5E-T-07 ST5E-T-11",
    "offline": "TC-19-I TC-21 S4C-017-I ST5E-T-02 ST5E-T-03",
    "source-ink": "TC-04 CF-22 SV-10 S4C-004 S4C-023",
    "modes": "CF-23 S4C-001",
    "recognition": "TC-05 SV-04 ST5E-T-04 ST5E-T-05",
    "model-lifecycle": "CF-24 SV-09 SV-26 S4C-024 ST5E-T-08 ST5E-T-09 ST5E-T-10 ST5E-T-13",
    "organisation": "TC-07-I CF-01 CF-02 SV-27-I S4C-003 S4C-019-I S4C-027-I ST5B-I1-01 ST5B-I1-02 ST5B-I1-03 ST5B-I1-04 ST5E-T-14 ST5E-T-15 ST5E-T-16 ST5E-T-17 ST5E-T-18",
    "exercises": "CF-25 CF-28 S4C-002 S4C-026",
    "rich-objects": "SV-01 SV-03 SV-13 SV-16 S4C-007",
    "pdf-sources": "TC-17-I TC-18 TC-26-I CF-06 SV-05 ST5B-I1-06 ST5B-I1-08 ST5E-T-19",
    "export": "TC-25-I SV-20-I S4C-028-I ST5E-T-20 ST5E-T-21 ST5E-T-22",
    "recovery": "TC-06-I ST5B-I1-05",
    "security": "CF-26 SV-02 SV-21 S4C-008 ST5B-I1-07 ST5E-T-12",
    "governance": "TC-24",
    "post-hi": "CF-29 CF-30 ST5E-T-26 ST5E-T-27",
}


def rows(name):
    return list(csv.DictReader((SOURCE / name).open()))


def tests(module):
    if module.startswith("unit:"):
        _, file, namespace, name = module.split(":", 3)
        relative = f"core/src/{file}"
        source = (ROOT / relative).read_text()
        assert re.search(r"mod\s+" + re.escape(namespace) + r"\s*\{", source)
        assert re.search(r"#\[test\]\s*fn\s+" + re.escape(name) + r"\s*\(", source)
        return [f"{relative}::{namespace}::{name}"]
    if module.startswith("android:"):
        name = module.split(":", 1)[1]
        relative = f"android/backend/src/test/java/org/stemnotebook/backend/{name}.kt"
        names = re.findall(r"@Test(?:\([^)]*\))?\s+fun (\w+)", (ROOT / relative).read_text())
    else:
        relative = f"core/tests/{module}.rs"
        names = re.findall(r"#\[test\]\s*fn (\w+)", (ROOT / relative).read_text())
    assert names, f"No executable fixture methods found: {relative}"
    return [f"{relative}::{name}" for name in names]


def csv_text(data):
    buffer = io.StringIO(newline="")
    writer = csv.DictWriter(buffer, fieldnames=list(data[0]), lineterminator="\n")
    writer.writeheader()
    writer.writerows(data)
    return buffer.getvalue()


def generate():
    crosswalk = rows("Stage5D_94_Requirement_Release_Contract_CROSSWALK.csv")
    historical = rows("Stage5F_94_Requirement_to_Test_Link_Audit.csv")
    inherited = rows("Stage5F_103_Inherited_Clause_Audit.csv")
    candidates = rows("Stage5F_158_Test_Candidate_Provenance_Ledger.csv")
    assert (len(crosswalk), len(historical), len(inherited), len(candidates)) == (94, 94, 103, 158)
    requirement_groups = {r: g for g, values in GROUPS.items() for r in values[0].split()}
    candidate_groups = {c: g for g, members in CANDIDATES.items() for c in members.split()}
    assert len(candidate_groups) == sum(len(v.split()) for v in CANDIDATES.values())
    assert not (set(candidate_groups) - {r["check_id"] for r in candidates})
    requirement_ids = {r["requirement_id"] for r in crosswalk}
    assert requirement_ids == {r["requirement_id"] for r in historical}
    assert not (set(requirement_groups) - requirement_ids)
    spec_path = ROOT / "docs/spec/approved-v0.5.8.md"
    spec = spec_path.read_text()
    requirement_map = []
    for r in crosswalk:
        identifier = r["requirement_id"]
        group = requirement_groups.get(identifier, "later")
        if r["initial_release_obligation"] == "YES":
            assert group != "later", identifier
        if group == "later":
            code, refs, gap = [], [], "M1/M2/later authored feature; not silently brought into I0/I1. Shared source/access invariants for exposed I1 PDFs are tracked under pdf-sources/security."
            state = "LATER"
        else:
            _, implementation, modules, gap = GROUPS[group]
            code = [p if p.startswith(("android/", "bridge/")) else f"core/src/{p}" for p in implementation.split()]
            refs = sorted({t for m in modules.split() for t in tests(m)})
            state = "POST_HI" if group == "post-hi" else "PARTIAL" if refs or group == "governance" else "NOT_IMPLEMENTED"
        if identifier == "NB-SUP-001":
            refs = ["core/tests/writing.rs::search_matches_literal_accepted_content_never_raw_or_locked_data"]
        if identifier == "NB-SUP-002":
            refs = ["core/tests/converters.rs::offline_help_covers_recovery_and_unsearchable_ink_without_remote_dependency"]
        requirement_map.append({"requirement_id": identifier, "phase": r["stage5a_phase"], "initial_release": r["initial_release_obligation"], "source": "docs/spec/approved-v0.5.8.md", "normative_text": r["source_normative_behavior"], "fixture_family": group, "backend_state": state, "code_paths": ";".join(code), "related_executable_tests": ";".join(refs), "requirement_result": "NOT RUN", "coverage_limit": gap, "evidence": "docs/evidence/latest/validation.json (individual tests only; not whole-requirement acceptance)"})
    by_req = {r["requirement_id"]: r for r in requirement_map}
    inherited_map = []
    for r in inherited:
        old = r["legacy_id"]
        match = re.search(r"\| `" + re.escape(old) + r"`[^|]*\| ([^|]+)", spec)
        assert match, f"Missing original clause {old}"
        target = {"SUP-002": "NB-SUP-001", "SUP-003": "NB-SUP-002"}.get(old, r["current_requirement_id"])
        assert target in by_req
        inherited_map.append({"legacy_id": old, "historical_requirement": r["current_requirement_id"], "current_semantic_requirement": target, "original_text": match.group(1).strip(), "fixture_family": by_req[target]["fixture_family"], "semantic_closure": "NOT RUN", "related_executable_tests": by_req[target]["related_executable_tests"], "mapping_note": "Editorial mismatch corrected in current map only: original source text preserved." if target != r["current_requirement_id"] else "Historical editorial mapping retained for review; related tests do not certify every clause."})
    candidate_map = []
    for r in candidates:
        identifier = r["check_id"]
        group = candidate_groups.get(identifier)
        if group is None:
            assert r["phase"] in ("M1", "M2", "L"), f"Unmapped current candidate {identifier}"
            group = "later-" + r["phase"].lower()
        refs = sorted({t for m in GROUPS[group][2].split() for t in tests(m)}) if group in GROUPS else []
        candidate_map.append({"check_id": identifier, "source_parent": r["source_parent"], "source_phase": r["phase"], "current_fixture_family": group, "expected_result": r["expected_result"], "execution_result": "NOT RUN", "related_executable_tests": ";".join(refs), "dedup_disposition": "Shared fixture family; retain this complete assertion separately. No inferred PASS or distinct-test count.", "phase_note": "Source M2 label conflicts with I1 canvas-embed gesture semantics; carry I1 backend camera invariant and post-gate H.I. action, without rewriting source." if identifier == "SV-16" else "Owner-local manual checks deferred until runnable app." if group == "post-hi" else "Historical later-phase assertions do not waive exposed I1 source/privacy invariants." if group.startswith("later-") else "Related automated tests cover subsets; full source candidate not yet executed."})
    manifest = {"baseline_sha256": hashlib.sha256(spec_path.read_bytes()).hexdigest(), "canonical_rows": len(requirement_map), "inherited_rows": len(inherited_map), "candidate_rows": len(candidate_map), "fixture_families": sorted({r["current_fixture_family"] for r in candidate_map}), "note": "Rows and fixture families are not executed-test totals. Check validation.json for observed test execution."}
    return {"requirements-current.csv": csv_text(requirement_map), "inherited-current.csv": csv_text(inherited_map), "candidates-current.csv": csv_text(candidate_map), "traceability-manifest.json": json.dumps(manifest, indent=2) + "\n"}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    for name, content in generate().items():
        path = OUT / name
        if args.check:
            assert path.read_text() == content, f"Stale map: run python3 tools/traceability.py ({name})"
        else:
            path.write_text(content)
    print("Traceability intact: 94 requirements, 103 inherited clauses, 158 candidates; no candidate PASS inferred.")


if __name__ == "__main__":
    main()
