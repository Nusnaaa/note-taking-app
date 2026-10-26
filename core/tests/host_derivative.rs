mod common;
use common::*;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::Duration,
};
use stem_core::{
    content::Content,
    export::{Page, Selection},
    host::{
        Command, DerivativeFormat, DerivativeOutput, DerivativeReview, Host, NativeExportSelection,
        Request,
    },
    processing::JobControl,
    security::{Access, Authenticator, Scope},
    store::{Kind, Store},
    Error, Id,
};
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(60)).unwrap()
}
fn wire(command: Command) -> String {
    serde_json::to_string(&Request {
        version: 1,
        command,
    })
    .unwrap()
}
fn fixture() -> (tempfile::TempDir, Arc<Host>, Id, Id) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let (note, ink) = {
        let mut s = Store::open(&path).unwrap();
        let a = Access::public();
        let note = s.create(Kind::Note, None, "derivative", &a).unwrap();
        s.append_content(note.id, Content::Text("001 + 1 = 3 λ".into()), &a)
            .unwrap();
        let ink = s.append_ink(note.id, vec![stroke(0)], &a).unwrap();
        (note.id, ink.id)
    };
    (root, Arc::new(Host::open(path, 8).unwrap()), note, ink)
}
fn review(host: &Host, note: Id, ink: bool, control: &JobControl) -> DerivativeReview {
    host.render_derivative(
        host.derivative_source(Selection::Document(note), ink)
            .unwrap(),
        DerivativeFormat::Markdown,
        control,
    )
    .unwrap()
}
fn lock(host: &Host, note: Id) {
    host.authenticate(
        [Scope::Entity(note)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    let current = host
        .request(&wire(Command::Read { id: note }))
        .unwrap()
        .data
        .unwrap();
    assert!(
        host.request(&wire(Command::SetLock {
            entity: note,
            revision: current["revision"].as_u64().unwrap(),
            locked: true
        }))
        .unwrap()
        .ok
    );
}
#[test]
fn host_derivative_delivers_only_confirmed_output_with_explicit_original_choice() {
    let (_root, host, note, ink) = fixture();
    let r = review(&host, note, false, &control());
    assert!(!r.summary().unwrap().includes_original_ink);
    assert!(matches!(
        host.finish_derivative(r, &mut TestAuth(true), false),
        Err(Error::ReviewRequired)
    ));
    let r = review(&host, note, false, &control());
    let DerivativeOutput::Markdown(bundle) = host
        .finish_derivative(r, &mut TestAuth(true), true)
        .unwrap()
    else {
        panic!()
    };
    assert!(bundle.markdown.contains("001 + 1 = 3 λ"));
    assert!(bundle.assets.is_empty());
    let r = review(&host, note, true, &control());
    assert!(r.summary().unwrap().includes_original_ink);
    let DerivativeOutput::Markdown(bundle) = host
        .finish_derivative(r, &mut TestAuth(true), true)
        .unwrap()
    else {
        panic!()
    };
    let svg = String::from_utf8(bundle.assets[&format!("original-{ink}.svg")].clone()).unwrap();
    assert!(svg.contains("1.2345678901234567"));
}
#[test]
fn derivative_and_native_snapshots_share_bounded_admission_and_drop_releases_it() {
    let (_root, host, note, _) = fixture();
    let source = host
        .derivative_source(Selection::Document(note), false)
        .unwrap();
    assert!(matches!(
        host.prepare_native_export(NativeExportSelection::Entities(vec![note]), true),
        Err(Error::ResourceLimit)
    ));
    drop(source);
    let native = host
        .prepare_native_export(NativeExportSelection::Entities(vec![note]), true)
        .unwrap();
    assert!(matches!(
        host.derivative_source(Selection::Document(note), false),
        Err(Error::ResourceLimit)
    ));
    drop(native);
    drop(review(&host, note, false, &control()));
    drop(review(&host, note, false, &control()));
}
#[test]
fn derivative_release_rechecks_generation_host_and_retained_cancellation() {
    let (_root, host, note, _) = fixture();
    let r = review(&host, note, false, &control());
    assert!(
        host.request(&wire(Command::Append {
            entity: note,
            content: Content::Text("later edit".into())
        }))
        .unwrap()
        .ok
    );
    assert!(matches!(
        host.finish_derivative(r, &mut TestAuth(true), true),
        Err(Error::Conflict)
    ));
    let (_other, other, _, _) = fixture();
    let source = host
        .derivative_source(Selection::Document(note), false)
        .unwrap();
    assert!(matches!(
        other.render_derivative(source, DerivativeFormat::Markdown, &control()),
        Err(Error::Denied)
    ));
    let r = review(&host, note, false, &control());
    assert!(matches!(
        other.finish_derivative(r, &mut TestAuth(true), true),
        Err(Error::Denied)
    ));
    let c = control();
    let r = review(&host, note, false, &c);
    c.cancel();
    assert!(matches!(r.summary(), Err(Error::Cancelled)));
    assert!(matches!(
        host.finish_derivative(r, &mut TestAuth(true), true),
        Err(Error::Cancelled)
    ));
    drop(review(&host, note, false, &control()));
}
#[test]
fn relock_capture_and_close_refuse_derivative_source_or_review_without_output() {
    let (_root, host, note, _) = fixture();
    let source = host
        .derivative_source(Selection::Document(note), false)
        .unwrap();
    let pen = Id::new();
    host.set_capture_priority(pen, true).unwrap();
    assert!(matches!(
        host.render_derivative(source, DerivativeFormat::Markdown, &control()),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        host.derivative_source(Selection::Document(note), false),
        Err(Error::ResourceLimit)
    ));
    host.set_capture_priority(pen, false).unwrap();
    let r = review(&host, note, false, &control());
    host.request(&wire(Command::Relock {})).unwrap();
    assert!(matches!(
        host.finish_derivative(r, &mut TestAuth(true), true),
        Err(Error::Cancelled)
    ));
    let source = host
        .derivative_source(Selection::Document(note), false)
        .unwrap();
    host.close().unwrap();
    assert!(matches!(
        host.render_derivative(source, DerivativeFormat::Markdown, &control()),
        Err(Error::Cancelled) | Err(Error::WorkerFailed)
    ));
}
struct Reentrant {
    host: Arc<Host>,
    close: bool,
}
impl Authenticator for Reentrant {
    fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
        assert_eq!(self.host.capture_library_identity().unwrap().len(), 36);
        if self.close {
            assert!(matches!(self.host.close(), Err(Error::WorkerFailed)));
        }
        true
    }
}
#[test]
fn derivative_fresh_proof_is_off_writer_and_close_reentry_is_cancelled() {
    for close in [false, true] {
        let (_root, host, note, _) = fixture();
        lock(&host, note);
        let r = review(&host, note, false, &control());
        assert!(matches!(
            host.finish_derivative(r, &mut TestAuth(false), true),
            Err(Error::Denied)
        ));
        let r = review(&host, note, false, &control());
        let result = host.finish_derivative(
            r,
            &mut Reentrant {
                host: Arc::clone(&host),
                close,
            },
            true,
        );
        if close {
            assert!(matches!(result, Err(Error::Cancelled)));
            host.close().unwrap();
        } else {
            assert!(result.is_ok());
        }
    }
}
fn font() -> Vec<u8> {
    std::fs::read(
        std::env::var("STEM_TEST_FONT")
            .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
    )
    .unwrap()
}
#[test]
fn pdf_rendering_requires_local_font_and_review_and_refuses_original_ink_mode() {
    let (_root, host, note, _) = fixture();
    let r = host
        .render_derivative(
            host.derivative_source(Selection::Document(note), false)
                .unwrap(),
            DerivativeFormat::Pdf {
                font: &font(),
                page: Page::default(),
            },
            &control(),
        )
        .unwrap();
    assert!(r.summary().unwrap().pages.unwrap() >= 1);
    assert!(r
        .summary()
        .unwrap()
        .warnings
        .iter()
        .any(|s| s.contains("typeset content derivative")));
    let DerivativeOutput::Pdf(pdf) = host
        .finish_derivative(r, &mut TestAuth(true), true)
        .unwrap()
    else {
        panic!()
    };
    assert!(pdf.bytes.starts_with(b"%PDF-"));
    let source = host
        .derivative_source(Selection::Document(note), true)
        .unwrap();
    assert!(matches!(
        host.render_derivative(
            source,
            DerivativeFormat::Pdf {
                font: &font(),
                page: Page::default()
            },
            &control()
        ),
        Err(Error::Unsupported)
    ));
    let source = host
        .derivative_source(Selection::Document(note), false)
        .unwrap();
    assert!(host
        .render_derivative(
            source,
            DerivativeFormat::Pdf {
                font: b"invalid",
                page: Page::default()
            },
            &control()
        )
        .is_err());
    drop(review(&host, note, false, &control()));
}
struct ScopeProof(Arc<Mutex<BTreeSet<Scope>>>);
impl Authenticator for ScopeProof {
    fn verify(&mut self, _: Id, scopes: &BTreeSet<Scope>) -> bool {
        *self.0.lock().unwrap() = scopes.clone();
        true
    }
}
#[test]
fn empty_exercise_derivative_keeps_its_locked_master_in_fresh_proof() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("empty.db");
    let (note, exercise) = {
        let mut s = Store::open(&path).unwrap();
        let a = Access::public();
        let note = s.create(Kind::Note, None, "master", &a).unwrap();
        let e = s.create_exercise(note.id, "empty", &[], &a).unwrap();
        (note.id, e.id)
    };
    let host = Host::open(path, 8).unwrap();
    lock(&host, note);
    let source = host
        .derivative_source(Selection::Exercise(exercise), false)
        .unwrap();
    let r = host
        .render_derivative(source, DerivativeFormat::Markdown, &control())
        .unwrap();
    let seen = Arc::new(Mutex::new(BTreeSet::new()));
    host.finish_derivative(r, &mut ScopeProof(Arc::clone(&seen)), true)
        .unwrap();
    assert!(seen.lock().unwrap().contains(&Scope::Entity(note)));
}
#[test]
fn selected_shared_image_requires_all_actual_reference_scopes_at_release() {
    use stem_core::objects::Image;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("image.db");
    let (selected, private) = {
        let mut s = Store::open(&path).unwrap();
        let a = Access::public();
        let note = s.create(Kind::Note, None, "image", &a).unwrap();
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(1, 1))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let asset = s
            .import_image(note.id, "image/png", &bytes.into_inner(), true, &a)
            .unwrap();
        let content = Content::Image(Image {
            asset,
            caption: "selected image".into(),
            crop: None,
        });
        let selected = s.append_content(note.id, content.clone(), &a).unwrap();
        let private = s.append_content(note.id, content, &a).unwrap();
        s.set_block_lock(
            private.id,
            private.revision,
            true,
            &access([Scope::Block(private.id)]),
        )
        .unwrap();
        (selected.id, private.id)
    };
    let host = Host::open(path, 8).unwrap();
    assert!(matches!(
        host.derivative_source(Selection::Blocks(vec![selected]), false),
        Err(Error::Denied)
    ));
    host.authenticate(
        [Scope::Block(private)].into_iter().collect(),
        Box::new(TestAuth(true)),
    )
    .unwrap();
    let source = host
        .derivative_source(Selection::Blocks(vec![selected]), false)
        .unwrap();
    let r = host
        .render_derivative(source, DerivativeFormat::Markdown, &control())
        .unwrap();
    let seen = Arc::new(Mutex::new(BTreeSet::new()));
    host.finish_derivative(r, &mut ScopeProof(Arc::clone(&seen)), true)
        .unwrap();
    assert!(seen.lock().unwrap().contains(&Scope::Block(private)));
}

#[test]
fn selected_image_does_not_decode_unrelated_malformed_private_block_payload() {
    use stem_core::objects::Image;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private-image.db");
    let mut s = Store::open(&path).unwrap();
    let a = Access::public();
    let note = s.create(Kind::Note, None, "image", &a).unwrap();
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::new(1, 1))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let bytes = bytes.into_inner();
    let asset = s
        .import_image(note.id, "image/png", &bytes, true, &a)
        .unwrap();
    let selected = s
        .append_content(
            note.id,
            Content::Image(Image {
                asset,
                caption: "selected".into(),
                crop: None,
            }),
            &a,
        )
        .unwrap();
    let private = s
        .append_content(note.id, Content::Text("private text".into()), &a)
        .unwrap();
    s.set_block_lock(
        private.id,
        private.revision,
        true,
        &access([Scope::Block(private.id)]),
    )
    .unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE blocks SET payload=json_set(payload,'$.content',json('{}')) WHERE id=?1",
            [private.id.to_string()],
        )
        .unwrap();
    let plan = s
        .prepare_derivative(Selection::Blocks(vec![selected.id]), false, &a)
        .unwrap();
    let bundle = s
        .finish_derivative(plan, &mut TestAuth(true), true)
        .unwrap();
    assert_eq!(bundle.assets[&format!("image-{asset}.png")], bytes);
    assert!(matches!(s.block(private.id, &a), Err(Error::Denied)));
}

#[test]
fn excessive_svg_point_expansion_fails_without_truncated_derivative_or_source_changes() {
    let mut s = Store::memory().unwrap();
    let a = Access::public();
    let note = s
        .create(Kind::Note, None, "large coordinate derivative", &a)
        .unwrap();
    let mut ink = stroke(0);
    let p = ink.points[0].clone();
    ink.points = (0..30000)
        .map(|i| stem_core::ink::Point {
            x: 1e150,
            y: 1e150,
            time_us: i,
            ..p.clone()
        })
        .collect();
    let block = s.append_ink(note.id, vec![ink.clone()], &a).unwrap();
    assert!(matches!(
        s.prepare_derivative(Selection::Blocks(vec![block.id]), true, &a),
        Err(Error::ResourceLimit)
    ));
    let plan = s
        .prepare_derivative(Selection::Blocks(vec![block.id]), false, &a)
        .unwrap();
    assert!(!plan.includes_original_ink);
    assert_eq!(s.source_ink(block.id, &a).unwrap(), vec![ink]);
}

#[test]
fn empty_selection_under_app_lock_still_requires_fresh_app_proof() {
    let (_root, host, _, _) = fixture();
    host.authenticate([Scope::App].into_iter().collect(), Box::new(TestAuth(true)))
        .unwrap();
    assert!(
        host.request(&wire(Command::SetAppLock { enabled: true }))
            .unwrap()
            .ok
    );
    let source = host
        .derivative_source(Selection::Blocks(vec![]), false)
        .unwrap();
    let r = host
        .render_derivative(source, DerivativeFormat::Markdown, &control())
        .unwrap();
    assert!(matches!(
        host.finish_derivative(r, &mut TestAuth(false), true),
        Err(Error::Denied)
    ));
    let source = host
        .derivative_source(Selection::Blocks(vec![]), false)
        .unwrap();
    let r = host
        .render_derivative(source, DerivativeFormat::Markdown, &control())
        .unwrap();
    let seen = Arc::new(Mutex::new(BTreeSet::new()));
    host.finish_derivative(r, &mut ScopeProof(Arc::clone(&seen)), true)
        .unwrap();
    assert!(seen.lock().unwrap().contains(&Scope::App));
}
