mod common;
use common::*;
use stem_core::{
    capture::{InputRouter, PointerKind},
    ink::{PressureResponse, StrokeStyle},
};
fn response() -> PressureResponse {
    PressureResponse {
        input_min: 0.,
        input_max: 2.,
        minimum_factor: 0.25,
        maximum_factor: 1.25,
        exponent: 2.,
    }
}
fn style() -> StrokeStyle {
    StrokeStyle {
        colour_rgba: [32, 64, 128, 255],
        width: 8.,
        pressure_response: Some(response()),
    }
}
#[test]
fn absent_zero_outside_domain_and_interior_pressure_have_explicit_derived_widths() {
    let s = style();
    assert_eq!(s.width_for(None).unwrap(), 8.);
    assert_eq!(s.width_for(Some(0.)).unwrap(), 2.);
    assert_eq!(s.width_for(Some(1.)).unwrap(), 4.);
    assert_eq!(s.width_for(Some(2.)).unwrap(), 10.);
    assert_eq!(s.width_for(Some(-f64::MAX)).unwrap(), 2.);
    assert_eq!(s.width_for(Some(f64::MAX)).unwrap(), 10.);
    assert!(s.width_for(Some(f64::NAN)).is_err());
    let fixed = StrokeStyle {
        pressure_response: None,
        ..s
    };
    assert_eq!(fixed.width_for(Some(0.)).unwrap(), 8.);
}
#[test]
fn invalid_responses_and_effective_widths_fail_without_installing_them() {
    let r = response();
    for invalid in [
        PressureResponse { input_max: 0., ..r },
        PressureResponse {
            input_min: -f64::MAX,
            input_max: f64::MAX,
            ..r
        },
        PressureResponse { exponent: 0., ..r },
        PressureResponse {
            exponent: f64::NAN,
            ..r
        },
        PressureResponse {
            minimum_factor: -0.1,
            ..r
        },
        PressureResponse {
            minimum_factor: 2.,
            ..r
        },
    ] {
        assert!(invalid.validate().is_err());
    }
    let mut router = InputRouter::new("recorded-source", false, 10).unwrap();
    router.set_style(Some(style())).unwrap();
    assert!(router
        .set_style(Some(StrokeStyle {
            width: 1_000_000.,
            ..style()
        }))
        .is_err());
    router
        .begin(1, PointerKind::Pen, stroke(0).points[0].clone())
        .unwrap();
    assert_eq!(router.finish(1).unwrap().style, Some(style()));
}
#[test]
fn response_is_snapshotted_per_contact_and_raw_samples_are_never_rewritten() {
    let mut samples = stroke(0).points;
    samples[0].pressure = None;
    samples[1].pressure = Some(0.);
    let mut router = InputRouter::new("recorded-source", false, 10).unwrap();
    router.set_style(Some(style())).unwrap();
    router
        .begin(1, PointerKind::Pen, samples[0].clone())
        .unwrap();
    router
        .set_style(Some(StrokeStyle {
            pressure_response: None,
            ..style()
        }))
        .unwrap();
    router.sample(1, samples[1].clone()).unwrap();
    let recorded = router.finish(1).unwrap();
    let before = serde_json::to_vec(&recorded).unwrap();
    assert_eq!(recorded.style, Some(style()));
    assert_eq!(recorded.points, samples);
    for point in &recorded.points {
        recorded.style.unwrap().width_for(point.pressure).unwrap();
    }
    assert_eq!(before, serde_json::to_vec(&recorded).unwrap());
}

#[test]
fn original_svg_uses_recorded_response_and_reports_missing_sensor_values_without_rewriting_ink() {
    use stem_core::{
        export::Selection,
        security::Access,
        store::{Kind, Store},
    };
    let a = Access::public();
    let mut store = Store::memory().unwrap();
    let note = store.create(Kind::Note, None, "pressure", &a).unwrap();
    let mut recorded = stroke(0);
    recorded.style = Some(style());
    recorded.points[0].pressure = None;
    recorded.points[1].pressure = Some(0.);
    let mut final_point = recorded.points[1].clone();
    final_point.pressure = Some(2.);
    final_point.x += 2.;
    final_point.time_us += 1;
    recorded.points.push(final_point);
    for p in &mut recorded.points {
        p.tilt_rad = None;
        p.orientation_rad = None;
    }
    let block = store
        .append_ink(note.id, vec![recorded.clone()], &a)
        .unwrap();
    let bundle = store
        .finish_derivative(
            store
                .prepare_derivative(Selection::Blocks(vec![block.id]), true, &a)
                .unwrap(),
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    let svg = String::from_utf8(bundle.assets.values().next().unwrap().clone()).unwrap();
    assert!(svg.contains("r=\"4\""));
    assert!(svg.contains("r=\"1\""));
    assert!(svg.contains("r=\"5\""));
    assert!(svg.contains("#204080"));
    assert!(svg.contains("<polygon"));
    assert!(bundle
        .warnings
        .iter()
        .any(|s| s.contains("Pressure was unavailable")));
    assert!(!bundle
        .warnings
        .iter()
        .any(|s| s.contains("pressure/tilt-dependent")));
    assert_eq!(store.source_ink(block.id, &a).unwrap(), vec![recorded]);
}
#[test]
fn pressure_and_eraser_preferences_persist_and_new_fields_cannot_claim_an_old_native_version() {
    use stem_core::{
        portable::{ExportProtection, NativeDocument},
        security::Access,
        store::*,
        Id,
    };
    let a = Access::public();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pressure.db");
    let mut store = Store::open(&path).unwrap();
    let note = store
        .create(Kind::Note, None, "pressure prefs", &a)
        .unwrap();
    let view = store
        .save_view(
            ViewSettings {
                id: Id::new(),
                entity: note.id,
                exercise: None,
                camera: Camera {
                    centre_x: 0.,
                    centre_y: 0.,
                    zoom: 1.,
                    rotation_rad: 0.,
                },
                pen_width_mm: 0.5,
                colour_rgba: [0, 0, 0, 255],
                finger_ink: false,
                anchor: None,
                bookmark: None,
                authoring: None,
                revision: 0,
            },
            None,
            &a,
        )
        .unwrap();
    let preset = Id::new();
    let configured = store
        .configure_view(
            view.id,
            view.revision,
            Some(AuthoringSettings {
                presets: vec![PenPreset {
                    id: preset,
                    name: "chosen pressure".into(),
                    width_mm: 0.8,
                    colour_rgba: [32, 64, 128, 255],
                    pressure_response: Some(response()),
                }],
                selected_preset: Some(preset),
                background: None,
                pressure_response: None,
                eraser: Some(EraserSettings {
                    mode: EraserMode::Selection,
                    width_mm: 2.,
                }),
            }),
            &a,
        )
        .unwrap();
    let snapshot = store.view_pen(view.id, "source-units", 10., &a).unwrap();
    assert_eq!(snapshot.style.width, 8.);
    assert_eq!(snapshot.style.pressure_response, Some(response()));
    assert_eq!(
        snapshot.eraser.unwrap(),
        ViewEraserSnapshot {
            mode: EraserMode::Selection,
            width: 20.
        }
    );
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.view(view.id, &a).unwrap(), configured);
    let exported = store
        .finish_native_export(
            store.prepare_native_export(&[note.id], true, &a).unwrap(),
            ExportProtection::Plaintext {
                acknowledge_protected: false,
            },
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    let mut document = NativeDocument::decode(&exported.bytes).unwrap();
    assert_eq!(document.version, 10);
    document.version = 9;
    assert!(document.validate().is_err());
}

#[test]
fn aggregate_and_stream_backups_keep_exact_pressure_metadata_and_reject_schema_downgrade() {
    use std::time::Duration;
    use stem_core::{
        backup::BackupOptions,
        crypto::{self, Credential},
        processing::JobControl,
        security::Access,
        store::{Kind, Store},
    };
    let a = Access::public();
    let mut source = Store::memory().unwrap();
    let note = source
        .create(Kind::Note, None, "pressure backup", &a)
        .unwrap();
    let mut original = stroke(0);
    original.style = Some(style());
    original.points[0].pressure = None;
    original.points[1].pressure = Some(0.);
    let block = source
        .append_ink(note.id, vec![original.clone()], &a)
        .unwrap();
    let options = BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    };
    let password = "Explicit pressure metadata recovery passphrase";
    let archive = source
        .finish_backup(
            source.prepare_backup(options, &a).unwrap(),
            password,
            &mut TestAuth(true),
            true,
        )
        .unwrap();
    let restored = Store::restore_backup_password(&archive.bytes, password).unwrap();
    assert_eq!(
        restored.source_ink(block.id, &a).unwrap(),
        vec![original.clone()]
    );
    let plain = crypto::decrypt(&archive.bytes, Credential::Password(password)).unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(&plain).unwrap();
    document["tables"]
        .as_object_mut()
        .unwrap()
        .remove("engine_resource_actions");
    document["schema"] = 15.into();
    let downgraded = crypto::encrypt(&serde_json::to_vec(&document).unwrap(), password).unwrap();
    assert!(Store::restore_backup_password(&downgraded.bytes, password).is_err());
    let control = JobControl::new(Duration::from_secs(300)).unwrap();
    let (bytes, key) = source
        .finish_stream_backup(
            source.prepare_stream_backup(options, &a).unwrap(),
            password,
            &mut TestAuth(true),
            true,
            Vec::new(),
            &control,
        )
        .unwrap();
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = builder.tempdir().unwrap();
    let stage = Store::restore_stream_backup(
        bytes.as_slice(),
        Credential::Recovery(&key),
        10000000,
        directory.path(),
        &control,
    )
    .unwrap();
    assert_eq!(
        stage.store().source_ink(block.id, &a).unwrap(),
        vec![original]
    );
}

#[test]
fn legacy_layout_stream_reads_schema15_without_inventing_pressure_settings() {
    use std::{
        io::{Read, Write},
        time::Duration,
    };
    use stem_core::{
        backup::BackupOptions,
        crypto::{
            stream::{DecryptStream, EncryptStream},
            Credential,
        },
        processing::JobControl,
        security::Access,
        store::{Kind, Store},
    };
    let a = Access::public();
    let mut source = Store::memory().unwrap();
    let note = source
        .create(Kind::Note, None, "legacy layout", &a)
        .unwrap();
    let mut original = stroke(0);
    original.style = Some(StrokeStyle {
        pressure_response: None,
        ..style()
    });
    let block = source
        .append_ink(note.id, vec![original.clone()], &a)
        .unwrap();
    let options = BackupOptions {
        include_original_ink: true,
        include_authorised_source_assets: false,
    };
    let password = "Explicit legacy layout compatibility passphrase";
    let control = JobControl::new(Duration::from_secs(300)).unwrap();
    let (bytes, _) = source
        .finish_stream_backup(
            source.prepare_stream_backup(options, &a).unwrap(),
            password,
            &mut TestAuth(true),
            true,
            Vec::new(),
            &control,
        )
        .unwrap();
    // Synthetic schema15 fixture has no pressure/eraser fields. Schema17 adds
    // the final action table; remove its empty segment before changing the tag.
    let mut reader =
        DecryptStream::new(bytes.as_slice(), Credential::Password(password), 10000000).unwrap();
    let mut plain = Vec::new();
    reader.read_to_end(&mut plain).unwrap();
    reader.finish().unwrap();
    assert_eq!(&plain[..8], b"STEMLIB2");
    let end = plain.len() - 16; // Footer magic plus the eight-byte row count.
    assert_eq!(&plain[end..end + 8], b"STEMEND2");
    assert_eq!(&plain[end - 3..end], &[0, 31, 0]); // Empty schema17 action table.
    plain.drain(end - 3..end);
    plain[8..12].copy_from_slice(&15u32.to_be_bytes());
    let mut writer = EncryptStream::new(Vec::new(), password).unwrap();
    writer.write_all(&plain).unwrap();
    let (legacy, _) = writer.finish().unwrap();
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = builder.tempdir().unwrap();
    let restored = Store::restore_stream_backup(
        legacy.as_slice(),
        Credential::Password(password),
        10000000,
        directory.path(),
        &control,
    )
    .unwrap();
    assert_eq!(
        restored.store().source_ink(block.id, &a).unwrap(),
        vec![original]
    );
    assert_eq!(restored.store().schema_version().unwrap(), 17);
}

#[test]
fn eraser_source_unit_underflow_is_rejected_without_changing_preferences() {
    use stem_core::{security::Access, store::*, Id};
    let a = Access::public();
    let mut store = Store::memory().unwrap();
    let note = store
        .create(Kind::Note, None, "eraser conversion", &a)
        .unwrap();
    let view = store
        .save_view(
            ViewSettings {
                id: Id::new(),
                entity: note.id,
                exercise: None,
                camera: Camera {
                    centre_x: 0.,
                    centre_y: 0.,
                    zoom: 1.,
                    rotation_rad: 0.,
                },
                pen_width_mm: 1.,
                colour_rgba: [0, 0, 0, 255],
                finger_ink: false,
                anchor: None,
                bookmark: None,
                authoring: None,
                revision: 0,
            },
            None,
            &a,
        )
        .unwrap();
    let configured = store
        .configure_view(
            view.id,
            view.revision,
            Some(AuthoringSettings {
                presets: Vec::new(),
                selected_preset: None,
                background: None,
                pressure_response: None,
                eraser: Some(EraserSettings {
                    mode: EraserMode::WholeStroke,
                    width_mm: f64::MIN_POSITIVE,
                }),
            }),
            &a,
        )
        .unwrap();
    assert!(store
        .view_pen(view.id, "declared-source", f64::MIN_POSITIVE, &a)
        .is_err());
    assert_eq!(store.view(view.id, &a).unwrap(), configured);
}
