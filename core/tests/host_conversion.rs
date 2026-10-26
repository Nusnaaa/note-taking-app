mod common;
use serde_json::Value;
use std::{
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
use stem_core::{
    content::Content,
    converters::TextConverter,
    host::{Command, Host, Request},
    processing::*,
    store::Kind,
    Error, Id, Result,
};
fn call(host: &Host, command: Command) -> Value {
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
fn fixture() -> (tempfile::TempDir, Arc<Host>, Id, Id) {
    let root = tempfile::tempdir().unwrap();
    let host = Arc::new(Host::open(root.path().join("library.db"), 8).unwrap());
    let note = Id::parse(
        call(
            &host,
            Command::Create {
                kind: Kind::Note,
                parent: None,
                title: "converter".into(),
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
                content: Content::Code {
                    text: "\t001\r\n1+1=3".into(),
                    language: None,
                },
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    (root, host, note, block)
}
fn manager(adapter: Box<dyn Converter>) -> Manager {
    let mut manager = Manager::new();
    manager
        .register_converter(
            Descriptor {
                id: "converter".into(),
                version: "1".into(),
                adapter_version: "1".into(),
                family: Family::Converter,
                input: InputContract::EditableV1,
                tasks: vec![],
                internals: Internals::StrokeOnlyVerified,
                offline: true,
                content_egress: false,
                personal_use_evidence: Some("synthetic test adapter only".into()),
                redistributable_resources: false,
                resource_sha256: None,
                runtime_contract: "stem-processing-v1".into(),
            },
            adapter,
        )
        .unwrap();
    manager.activate_local("converter", None, true).unwrap();
    manager
}
fn control() -> JobControl {
    JobControl::new(Duration::from_secs(30)).unwrap()
}
#[test]
fn derivative_preserves_lexemes_source_identity_and_runtime_revocation() {
    let (_root, host, _, block) = fixture();
    let mut manager = manager(Box::new(TextConverter));
    let source = host.conversion_source(block, None).unwrap();
    let identity = source.source();
    let completed = source
        .prepare(&manager, "converter", Format::PlainText)
        .unwrap()
        .run(&control())
        .unwrap();
    assert_eq!(completed.source(), identity);
    assert_eq!(completed.output().unwrap().text, "\t001\r\n1+1=3");
    assert!(completed.output().unwrap().fidelity.complete);
    assert!(!completed.output().unwrap().fidelity.lexical_change);
    manager.remove("converter").unwrap();
    assert!(matches!(completed.output(), Err(Error::Cancelled)));
    host.close().unwrap();
}
#[test]
fn capture_relock_and_close_revoke_prepared_and_delivered_conversion() {
    let (_root, host, _, block) = fixture();
    let manager = manager(Box::new(TextConverter));
    let prepared = host
        .conversion_source(block, None)
        .unwrap()
        .prepare(&manager, "converter", Format::PlainText)
        .unwrap();
    let contact = Id::new();
    host.set_capture_priority(contact, true).unwrap();
    assert!(matches!(prepared.run(&control()), Err(Error::Cancelled)));
    assert!(matches!(
        host.conversion_source(block, None),
        Err(Error::ResourceLimit)
    ));
    host.set_capture_priority(contact, false).unwrap();
    let completed = host
        .conversion_source(block, None)
        .unwrap()
        .prepare(&manager, "converter", Format::PlainText)
        .unwrap()
        .run(&control())
        .unwrap();
    call(&host, Command::Relock {});
    assert!(matches!(completed.output(), Err(Error::Cancelled)));
    let completed = host
        .conversion_source(block, None)
        .unwrap()
        .prepare(&manager, "converter", Format::PlainText)
        .unwrap()
        .run(&control())
        .unwrap();
    host.close().unwrap();
    assert!(matches!(completed.output(), Err(Error::Cancelled)));
}
struct Blocking {
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl Converter for Blocking {
    fn formats(&self) -> Vec<Format> {
        vec![Format::PlainText]
    }
    fn convert(
        &self,
        content: &Content,
        target: Format,
        control: &JobControl,
    ) -> Result<Converted> {
        self.entered.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        control.check()?;
        TextConverter.convert(content, target, control)
    }
}
#[test]
fn running_callback_is_off_writer_and_observes_capture_cancellation() {
    let (_root, host, note, block) = fixture();
    let (entered, waiting) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let manager = manager(Box::new(Blocking {
        entered,
        release: Mutex::new(released),
    }));
    let work = host
        .conversion_source(block, None)
        .unwrap()
        .prepare(&manager, "converter", Format::PlainText)
        .unwrap();
    let worker = std::thread::spawn(move || work.run(&control()));
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    call(
        &host,
        Command::Append {
            entity: note,
            content: Content::Text("saved while converting".into()),
        },
    );
    host.set_capture_priority(Id::new(), true).unwrap();
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    assert_eq!(host.recognition_status().reserved, 0);
    host.close().unwrap();
}

#[test]
fn source_admission_is_shared_bounded_and_denied_blocks_stay_denied() {
    let (_root, host, _, block) = fixture();
    let sources: Vec<_> = (0..8)
        .map(|_| host.conversion_source(block, None).unwrap())
        .collect();
    assert!(matches!(
        host.conversion_source(block, None),
        Err(Error::ResourceLimit)
    ));
    assert!(matches!(
        host.activation_source(),
        Err(Error::ResourceLimit)
    ));
    drop(sources);
    // Close waits for actual retirement even if a caller never prepared work.
    host.close().unwrap();
    assert_eq!(host.recognition_status().reserved, 0);
}

#[test]
fn protection_change_revokes_snapshot_and_refuses_new_source() {
    let (_root, host, _, block) = fixture();
    host.authenticate(
        [stem_core::security::Scope::Block(block)]
            .into_iter()
            .collect(),
        Box::new(common::TestAuth(true)),
    )
    .unwrap();
    let source = host.conversion_source(block, None).unwrap();
    let revision = source.source().2;
    call(
        &host,
        Command::SetBlockLock {
            block,
            revision,
            locked: true,
        },
    );
    assert!(matches!(
        source.prepare(
            &manager(Box::new(TextConverter)),
            "converter",
            Format::PlainText
        ),
        Err(Error::Cancelled)
    ));
    call(&host, Command::Relock {});
    assert!(matches!(
        host.conversion_source(block, None),
        Err(Error::Denied)
    ));
    host.close().unwrap();
}

#[test]
fn typed_table_cell_conversion_uses_its_stored_value_without_requiring_ink() {
    use stem_core::objects::{Cell, CellValue, Table};
    let (_root, host, note, _) = fixture();
    let cell = Id::new();
    let block = Id::parse(
        call(
            &host,
            Command::Append {
                entity: note,
                content: Content::Table(Table {
                    rows: vec![vec![Cell {
                        id: cell,
                        value: CellValue::Numeric("001".into()),
                    }]],
                    header_rows: 0,
                    layout: None,
                }),
            },
        )["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let manager = manager(Box::new(TextConverter));
    let completed = host
        .conversion_source(block, Some(cell))
        .unwrap()
        .prepare(&manager, "converter", Format::PlainText)
        .unwrap()
        .run(&control())
        .unwrap();
    assert_eq!(completed.source(), (block, Some(cell), 0));
    assert_eq!(completed.output().unwrap().text, "001");
    assert!(matches!(
        host.conversion_source(block, Some(Id::new())),
        Err(Error::Unavailable)
    ));
    host.close().unwrap();
}
