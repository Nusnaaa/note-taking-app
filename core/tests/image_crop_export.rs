mod common;
use common::*;
use std::io::Cursor;
use stem_core::{
    content::Content,
    export::Selection,
    objects::{Image, Rect},
    security::{Access, Scope},
    store::{Kind, Store},
    Error,
};
fn png(width: u32, height: u32) -> Vec<u8> {
    let pixels = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([(x * 50 % 256) as u8, (y * 80 % 256) as u8, 17, 255])
    });
    let mut output = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}
fn crop(x: f64, width: f64) -> Rect {
    Rect {
        x,
        y: 0.,
        width,
        height: 1.,
    }
}
fn fixture(
    bytes: &[u8],
    mime: &str,
    crop: Rect,
) -> (Store, stem_core::Id, stem_core::Id, stem_core::Id) {
    let a = Access::public();
    let mut s = Store::memory().unwrap();
    let n = s.create(Kind::Note, None, "image", &a).unwrap();
    let asset = s.import_image(n.id, mime, bytes, true, &a).unwrap();
    let b = s
        .append_content(
            n.id,
            Content::Image(Image {
                asset,
                caption: "crop [caption]".into(),
                crop: Some(crop),
            }),
            &a,
        )
        .unwrap();
    (s, n.id, b.id, asset)
}
#[test]
fn crop_emits_only_selected_pixels_and_strips_original_trailing_bytes_without_rewriting_asset() {
    let a = Access::public();
    let mut original = png(4, 2);
    let marker = b"PRIVATE_ORIGINAL_TRAILING_METADATA_837142";
    original.extend_from_slice(marker);
    let (s, n, _, asset) = fixture(&original, "image/png", crop(0.25, 0.5));
    let plan = s
        .prepare_derivative(Selection::Document(n), false, &a)
        .unwrap();
    let bundle = s
        .finish_derivative(plan, &mut TestAuth(true), true)
        .unwrap();
    assert_eq!(bundle.assets.len(), 1);
    let bytes = bundle.assets.values().next().unwrap();
    let decoded = image::load_from_memory(bytes).unwrap().to_rgba8();
    assert_eq!(decoded.dimensions(), (2, 2));
    for y in 0..2 {
        for x in 0..2 {
            assert_eq!(
                decoded.get_pixel(x, y),
                &image::Rgba([((x + 1) * 50) as u8, (y * 80) as u8, 17, 255])
            );
        }
    }
    assert!(!bytes.windows(marker.len()).any(|w| w == marker));
    assert_eq!(s.image_bytes(asset, &a).unwrap().1, original);
    assert!(bundle.warnings.iter().any(|w| w.contains("8-bit RGBA")));
}
#[test]
fn two_crops_of_one_asset_have_distinct_outputs_and_explicit_source_ink_is_still_included() {
    let a = Access::public();
    let original = png(4, 2);
    let (mut s, n, first, asset) = fixture(&original, "image/png", crop(0., 0.25));
    let raw = stroke(0);
    let b = s.append_ink(n, vec![raw.clone()], &a).unwrap();
    s.edit_content(
        b.id,
        b.revision,
        Content::Image(Image {
            asset,
            caption: "right".into(),
            crop: Some(crop(0.75, 0.25)),
        }),
        &a,
    )
    .unwrap();
    let plan = s
        .prepare_derivative(Selection::Blocks(vec![first, b.id]), true, &a)
        .unwrap();
    assert!(plan.includes_original_ink);
    let bundle = s
        .finish_derivative(plan, &mut TestAuth(true), true)
        .unwrap();
    let pictures = bundle
        .assets
        .iter()
        .filter(|(n, _)| n.ends_with(".png"))
        .collect::<Vec<_>>();
    assert_eq!(pictures.len(), 2);
    assert_eq!(bundle.assets.len(), 3);
    assert_ne!(pictures[0].0, pictures[1].0);
    let mut colours = pictures
        .iter()
        .map(|(_, b)| {
            image::load_from_memory(b)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0[0]
        })
        .collect::<Vec<_>>();
    colours.sort();
    assert_eq!(colours, vec![0, 150]);
    assert_eq!(s.source_ink(b.id, &a).unwrap(), vec![raw]);
}
#[test]
fn fractional_crop_rounds_inward_and_a_crop_with_no_complete_pixel_requires_review() {
    let a = Access::public();
    let bytes = png(4, 2);
    let (s, n, _, _) = fixture(&bytes, "image/png", crop(0.1, 0.8));
    let plan = s
        .prepare_derivative(Selection::Document(n), false, &a)
        .unwrap();
    let bundle = s
        .finish_derivative(plan, &mut TestAuth(true), true)
        .unwrap();
    assert!(bundle.warnings.iter().any(|w| w.contains("rounded inward")));
    assert_eq!(
        image::load_from_memory(bundle.assets.values().next().unwrap())
            .unwrap()
            .width(),
        2
    );
    let (s, n, _, _) = fixture(&bytes, "image/png", crop(0.1, 0.1));
    assert!(matches!(
        s.prepare_derivative(Selection::Document(n), false, &a),
        Err(Error::ReviewRequired)
    ));
}
#[test]
fn malformed_and_oversized_images_fail_without_bypassing_fresh_block_protection() {
    let a = Access::public();
    let malformed = b"\x89PNG\r\n\x1a\nmalformed";
    let (mut s, n, b, asset) = fixture(malformed, "image/png", crop(0., 1.));
    assert!(s
        .prepare_derivative(Selection::Document(n), false, &a)
        .is_err());
    assert_eq!(s.image_bytes(asset, &a).unwrap().1, malformed);
    s.set_block_lock(b, 0, true, &access([Scope::Block(b)]))
        .unwrap();
    assert!(matches!(
        s.prepare_derivative(Selection::Document(n), false, &a),
        Err(Error::Denied)
    ));
    let bytes = png(20000, 1);
    let (s, n, _, _) = fixture(&bytes, "image/png", crop(0., 1.));
    assert!(matches!(
        s.prepare_derivative(Selection::Document(n), false, &a),
        Err(Error::ResourceLimit)
    ));
}
#[test]
fn jpeg_crop_encodes_png_but_nonidentity_exif_orientation_requires_review() {
    use image::ImageEncoder;
    let a = Access::public();
    let pixels = image::RgbImage::from_fn(4, 2, |x, _| image::Rgb([(x * 50) as u8, 20, 30]));
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .write_image(&pixels, 4, 2, image::ExtendedColorType::Rgb8)
        .unwrap();
    let (s, n, _, _) = fixture(&bytes, "image/jpeg", crop(0.25, 0.5));
    let plan = s
        .prepare_derivative(Selection::Document(n), false, &a)
        .unwrap();
    let bundle = s
        .finish_derivative(plan, &mut TestAuth(true), true)
        .unwrap();
    let output = bundle.assets.values().next().unwrap();
    assert!(output.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(image::load_from_memory(output).unwrap().width(), 2);
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let mut oriented = bytes[..2].to_vec();
    oriented.extend_from_slice(b"\xff\xe1");
    oriented.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    oriented.extend_from_slice(exif);
    oriented.extend_from_slice(&bytes[2..]);
    let (s, n, _, _) = fixture(&oriented, "image/jpeg", crop(0.25, 0.5));
    assert!(matches!(
        s.prepare_derivative(Selection::Document(n), false, &a),
        Err(Error::ReviewRequired)
    ));
}
