mod common;
use common::*;
use quickdraw::{
    Container, ErrorCode, Fidelity, FontBook, Limits, OutlineEncoding, OutlineFont,
    OutlineSelection, RenderOptions, VectorFormat, export_vector, parse, render,
};
use std::{fs, process::Command};

const FONT: &[u8] = include_bytes!("fixtures/portable-outline.ttf");
const TTC: &[u8] = include_bytes!("fixtures/portable-outline.ttc");
fn selection(index: u32, kern: bool) -> OutlineSelection {
    OutlineSelection {
        face_index: index,
        encoding: OutlineEncoding::Macroman,
        kerning: kern,
    }
}
fn options(kern: bool) -> RenderOptions {
    let limits = Limits::default();
    let mut fonts = FontBook::default();
    fonts
        .insert_outline(
            7,
            10,
            0,
            OutlineFont::parse(FONT, selection(0, kern), &limits).unwrap(),
            &limits,
        )
        .unwrap();
    RenderOptions {
        fonts,
        fidelity: Fidelity::Approximate,
        ..Default::default()
    }
}
fn picture() -> Pict {
    let mut p = Pict::new(2, 60, 24);
    p.op(3, &word(7)).op(0x0d, &word(10)).op(0x15, &word(0));
    p
}
fn text(p: &mut Pict, x: i16, y: i16, bytes: &[u8]) {
    let mut data = words(&[y, x]);
    data.push(bytes.len() as u8);
    data.extend(bytes);
    p.op(0x28, &data);
}
fn pixels(p: Pict, options: &RenderOptions) -> Vec<u8> {
    render(
        &parse(&p.finish(), Container::Raw, &options.limits).unwrap(),
        options,
    )
    .unwrap()
    .image
    .rgb()
    .to_vec()
}
fn at(rgb: &[u8], x: usize, y: usize) -> &[u8] {
    &rgb[(y * 60 + x) * 3..(y * 60 + x) * 3 + 3]
}

#[test]
fn explicit_macroman_maps_accent_trademark_and_private_apple_without_substitution() {
    let mut p = picture();
    text(&mut p, 1, 10, &[b'A', 0x8e, 0xaa, 0xf0]);
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let r = render(&pic, &options(false)).unwrap();
    for x in [1, 8, 15, 22] {
        assert_eq!(at(r.image.rgb(), x, 4), &[0; 3]);
    }
    assert_eq!(at(r.image.rgb(), 7, 4), &[255; 3]);
    assert!(r.diagnostics.iter().any(|d|d.code=="explicit_outline_font" && d.message.contains("portable_outline_v1")));
    let mut strict = options(false);
    strict.fidelity = Fidelity::Strict;
    assert_eq!(
        render(&pic, &strict).unwrap_err().code,
        ErrorCode::Unsupported
    );
}

#[test]
fn glyph_advances_kerning_and_disabled_ligatures_determine_graphics_pen() {
    for (bytes, kern, advance) in [
        (b"AV".as_slice(), false, 14),
        (b"AV", true, 13),
        (b"fi", false, 14),
        (&[0xde], false, 7),
    ] {
        let mut p = picture();
        text(&mut p, 0, 10, bytes);
        p.op(0x21, &words(&[10, advance + 1]));
        let rgb = pixels(p, &options(kern));
        assert_eq!(at(&rgb, advance as usize, 10), &[0; 3]);
        assert_eq!(at(&rgb, advance as usize - 1, 10), &[255; 3]);
    }
}

#[test]
fn ratios_space_extra_and_character_extra_share_layout_and_pen_progression() {
    let mut p = picture();
    p.op(0x10, &words(&[1, 2, 1, 1]))
        .op(6, &0x8000i32.to_be_bytes())
        .op(0x16, &word(1));
    text(&mut p, 0, 10, b"A A");
    p.op(0x21, &words(&[10, 48]));
    let rgb = pixels(p, &options(false));
    assert_eq!(at(&rgb, 0, 3), &[0; 3]);
    assert_eq!(at(&rgb, 11, 9), &[0; 3]);
    assert_eq!(at(&rgb, 12, 5), &[255; 3]);
    assert_eq!(at(&rgb, 31, 5), &[0; 3]);
    assert_eq!(at(&rgb, 47, 10), &[0; 3]);
    assert_eq!(at(&rgb, 46, 10), &[255; 3]);
    let mut p = picture();
    p.op(0x10, &words(&[1, 1, 1, 2])).op(0x15, &word(0x4000));
    text(&mut p, -2, 10, b"A");
    p.op(0x21, &words(&[10, 2]));
    let rgb = pixels(p, &options(false));
    assert_eq!(at(&rgb, 0, 5), &[0; 3]);
    assert_eq!(at(&rgb, 1, 5), &[255; 3]);
    assert_eq!(at(&rgb, 1, 10), &[0; 3]);
}

#[test]
fn contours_holes_overlaps_and_xor_composite_once_per_run() {
    let mut p = picture();
    text(&mut p, 1, 10, b"O");
    let rgb = pixels(p, &options(false));
    assert_eq!(at(&rgb, 1, 4), &[0; 3]);
    assert_eq!(at(&rgb, 3, 6), &[255; 3]);
    let mut p = picture();
    p.op(5, &word(2)).op(0x16, &word(-7));
    text(&mut p, 1, 10, b"AA");
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let rgb = render(&pic, &options(false)).unwrap().image.rgb().to_vec();
    assert_eq!(at(&rgb, 2, 4), &[0; 3]);
    assert_eq!(
        export_vector(&pic, &options(false), 72., VectorFormat::Svg)
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}

#[test]
fn outline_vector_paths_are_real_curves_not_images_or_text_substitutions() {
    let mut p = picture();
    text(&mut p, 1, 12, b"CO");
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let svg = String::from_utf8(
        export_vector(&pic, &options(false), 72., VectorFormat::Svg)
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert!(svg.contains("C"));
    assert!(svg.contains("fill-rule=\"nonzero\""));
    assert!(!svg.contains("<image"));
    assert!(!svg.contains("<text"));
    assert!(!svg.contains("stroke="));
    let pdf = export_vector(&pic, &options(false), 72., VectorFormat::Pdf)
        .unwrap()
        .bytes;
    assert!(pdf.windows(3).any(|w| w == b" c\n"));
}

#[test]
fn origin_clip_and_delta_text_use_previous_start_not_advanced_pen() {
    let mut p = picture();
    p.op(0x0c, &words(&[2, 1]))
        .op(1, &region([1, 4, 20, 15], &[]));
    text(&mut p, 2, 11, b"A");
    p.op(0x29, &[7, 1, b'A']);
    let rgb = pixels(p, &options(false));
    assert_eq!(at(&rgb, 1, 4), &[255; 3]);
    assert_eq!(at(&rgb, 2, 4), &[0; 3]);
    assert_eq!(at(&rgb, 7, 4), &[0; 3]);
    assert_eq!(at(&rgb, 13, 4), &[255; 3]);
}

#[test]
fn exact_collection_face_style_and_duplicate_binding_are_enforced() {
    let limits = Limits::default();
    assert_eq!(
        OutlineFont::parse(FONT, selection(1, false), &limits)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        OutlineFont::parse(TTC, selection(2, false), &limits)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArguments
    );
    let mut opts = options(false);
    assert_eq!(
        opts.fonts
            .insert_outline(
                7,
                10,
                0,
                OutlineFont::parse(FONT, selection(0, false), &limits).unwrap(),
                &limits
            )
            .unwrap_err()
            .code,
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        opts.fonts
            .insert_outline(
                8,
                10,
                1,
                OutlineFont::parse(FONT, selection(0, false), &limits).unwrap(),
                &limits
            )
            .unwrap_err()
            .code,
        ErrorCode::InvalidArguments
    );
    opts.fonts
        .insert_outline(
            8,
            10,
            0,
            OutlineFont::parse(TTC, selection(1, false), &limits).unwrap(),
            &limits,
        )
        .unwrap();
    let mut p = picture();
    p.op(3, &word(8));
    text(&mut p, 0, 10, b"A");
    let rgb = pixels(p, &opts);
    assert_eq!(at(&rgb, 7, 4), &[0; 3]);
    assert_eq!(at(&rgb, 8, 4), &[255; 3]);
}

#[test]
fn missing_glyphs_controls_fonts_and_invalid_state_fail_observably() {
    for bytes in [b"Z".as_slice(), &[0], &[127]] {
        let mut p = picture();
        text(&mut p, 0, 10, bytes);
        let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
        let err = render(&pic, &options(false)).unwrap_err();
        assert_eq!(err.code, ErrorCode::Unsupported);
        assert_eq!(err.opcode, Some(0x28));
    }
    for (opcode, data) in [
        (3, word(9)),
        (0x2e, vec![0, 4, 1, 1, 0, 0]),
        (0x10, words(&[1, -1, 1, 1])),
        (0x2d, vec![0, 8, 1, 0, 0, 0, 0, 0, 0, 0]),
    ] {
        let mut p = picture();
        p.op(opcode, &data);
        text(&mut p, 0, 10, b"A");
        let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
        assert_eq!(
            render(&pic, &options(false)).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
    let mut p = picture();
    p.op(0x2e, &[0, 4, 1, 0, 0, 0]);
    text(&mut p, 0, 10, b"A");
    assert!(pixels(p, &options(false)).contains(&0));
}

#[test]
fn malformed_fonts_and_bounded_shaping_fail_instead_of_falling_back() {
    let limits = Limits::default();
    for n in [0, 4, 12, 80, FONT.len() / 2] {
        assert!(OutlineFont::parse(&FONT[..n], selection(0, false), &limits).is_err());
    }
    let mut bad = FONT.to_vec();
    bad[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(
        OutlineFont::parse(&bad, selection(0, false), &limits)
            .unwrap_err()
            .code,
        ErrorCode::Malformed
    );
    let table_offset = |tag: &[u8; 4]| {
        (0..u16::from_be_bytes([FONT[4], FONT[5]]) as usize)
            .find_map(|i| {
                let at = 12 + i * 16;
                (&FONT[at..at + 4] == tag)
                    .then(|| u32::from_be_bytes(FONT[at + 8..at + 12].try_into().unwrap()) as usize)
            })
            .unwrap()
    };
    let glyf = table_offset(b"glyf");
    let loca = table_offset(b"loca");
    // Fixture glyph 6 is an original composite of glyph A. Make it recursive.
    let offset = u16::from_be_bytes(FONT[loca + 12..loca + 14].try_into().unwrap()) as usize * 2;
    let mut cyclic = FONT.to_vec();
    cyclic[glyf + offset + 12..glyf + offset + 14].copy_from_slice(&6u16.to_be_bytes());
    assert_eq!(
        OutlineFont::parse(&cyclic, selection(0, false), &limits)
            .unwrap_err()
            .code,
        ErrorCode::Malformed
    );
    assert_eq!(
        OutlineFont::parse(
            FONT,
            selection(0, false),
            &Limits {
                max_input_bytes: 100,
                ..limits.clone()
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::ResourceLimit
    );
    let mut p = picture();
    text(&mut p, 0, 10, b"AAAA");
    let pic = parse(&p.finish(), Container::Raw, &limits).unwrap();
    for small in [
        Limits {
            max_work: 10000,
            ..limits.clone()
        },
        Limits {
            max_decoded_bytes: 5000,
            ..limits.clone()
        },
    ] {
        let mut opts = options(false);
        opts.limits = small;
        assert_eq!(
            render(&pic, &opts).unwrap_err().code,
            ErrorCode::ResourceLimit
        );
    }
}

#[test]
fn schema_one_outline_selectors_preserve_source_files_and_explicit_errors() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let input = root.join("input.pict");
    let font = root.join("font.ttf");
    let map = root.join("fonts.json");
    let output = root.join("out.svg");
    fs::write(&font, FONT).unwrap();
    let mut p = picture();
    text(&mut p, 0, 10, b"A");
    fs::write(&input, p.finish()).unwrap();
    for (selector, code) in [
        (serde_json::json!({"face_index":0,"encoding":"macroman"}), 0),
        (serde_json::json!({"face_index":1,"encoding":"macroman"}), 2),
        (
            serde_json::json!({"face_index":0,"encoding":"shift_jis"}),
            2,
        ),
        (serde_json::json!({"encoding":"macroman"}), 2),
    ] {
        fs::write(&map,serde_json::to_vec(&serde_json::json!({"schema":1,"fonts":[{"id":7,"size":10,"path":"font.ttf","outline":selector}]})).unwrap()).unwrap();
        let r = Command::new(env!("CARGO_BIN_EXE_quickdraw"))
            .args(["convert", "--input"])
            .arg(&input)
            .args(["--output"])
            .arg(&output)
            .args(["--font-map"])
            .arg(&map)
            .args([
                "--fidelity",
                "approximate",
                "--vector-policy",
                "preserve",
                "--overwrite",
            ])
            .output()
            .unwrap();
        assert_eq!(
            r.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&r.stderr)
        );
    }
    fs::write(&map,br#"{"schema":1,"fonts":[{"id":7,"size":10,"path":"absent.ttf","outline":{"face_index":0,"encoding":"macroman"}}]}"#).unwrap();
    let missing = Command::new(env!("CARGO_BIN_EXE_quickdraw"))
        .args(["convert", "--input"])
        .arg(&input)
        .args(["--output"])
        .arg(&output)
        .args(["--font-map"])
        .arg(&map)
        .args([
            "--overwrite",
            "--vector-policy",
            "preserve",
            "--fidelity",
            "approximate",
        ])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(3));
    fs::write(&map,br#"{"schema":1,"fonts":[{"id":7,"size":10,"path":"font.ttf","outline":{"face_index":0,"encoding":"macroman"}}]}"#).unwrap();
    let r = Command::new(env!("CARGO_BIN_EXE_quickdraw"))
        .args(["convert", "--input"])
        .arg(&input)
        .args(["--output"])
        .arg(&font)
        .args(["--format", "png", "--font-map"])
        .arg(&map)
        .args(["--overwrite", "--fidelity", "approximate"])
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(3));
    assert_eq!(fs::read(font).unwrap(), FONT);
}
