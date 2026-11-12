mod common;
use common::*;
use quickdraw::{BitmapFont, Container, ErrorCode, FontBook, Limits, RenderOptions, parse, render};
use std::{fs, process::Command};

fn options() -> RenderOptions {
    let mut fonts = FontBook::default();
    let limits = Limits::default();
    fonts
        .insert(
            7,
            3,
            0,
            BitmapFont::parse(&nfnt(), &limits).unwrap(),
            &limits,
        )
        .unwrap();
    RenderOptions {
        fonts,
        ..Default::default()
    }
}
fn picture() -> Pict {
    let mut p = Pict::new(2, 12, 8);
    p.op(3, &word(7)).op(0x0d, &word(3));
    p
}
fn at(data: &[u8], x: usize, y: usize) -> &[u8] {
    &data[(y * 12 + x) * 3..(y * 12 + x) * 3 + 3]
}

#[test]
fn explicit_nfnt_metrics_baseline_bearing_and_advance_are_used() {
    let mut p = picture();
    p.op(0x28, &[0, 3, 0, 0, 2, b'A', b'B']);
    let r = render(
        &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
        &options(),
    )
    .unwrap();
    assert_eq!(at(r.image.rgb(), 0, 0), &[255; 3]);
    assert_eq!(at(r.image.rgb(), 1, 0), &[0; 3]);
    assert_eq!(at(r.image.rgb(), 5, 0), &[0; 3]); // advance 4 + left bearing 1
    assert_eq!(at(r.image.rgb(), 4, 0), &[255; 3]);
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == "explicit_bitmap_font")
    );
}

#[test]
fn text_deltas_use_previous_start_and_lines_use_advanced_graphics_pen() {
    let mut p = picture();
    p.op(0x28, &[0, 3, 0, 0, 1, b'A']);
    p.op(0x29, &[2, 1, b'A']); // baseline x=2, not advanced x=4 plus 2
    p.op(0x2a, &[3, 1, b'A']); // same starting x=2, y=6
    p.op(0x21, &words(&[6, 8])); // text advanced pen is (6,6)
    let r = render(
        &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
        &options(),
    )
    .unwrap();
    assert_eq!(at(r.image.rgb(), 3, 0), &[0; 3]);
    assert_eq!(at(r.image.rgb(), 3, 3), &[0; 3]);
    assert_eq!(at(r.image.rgb(), 6, 6), &[0; 3]);
    assert_eq!(at(r.image.rgb(), 5, 6), &[255; 3]);
}

#[test]
fn origin_clip_and_text_bic_use_existing_destination() {
    let mut p = picture();
    p.op(0x31, &rect(0, 0, 8, 12)).op(5, &word(3));
    p.op(0x0c, &words(&[2, 1]))
        .op(1, &region([1, 3, 4, 4], &[]));
    p.op(0x28, &[0, 4, 0, 2, 1, b'A']);
    let r = render(
        &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
        &options(),
    )
    .unwrap();
    assert_eq!(at(r.image.rgb(), 1, 0), &[255; 3]);
    assert_eq!(at(r.image.rgb(), 0, 1), &[0; 3]);
}

#[test]
fn absent_faces_sizes_and_glyphs_do_not_substitute() {
    for (opcode, data) in [(4, vec![1]), (0x0d, word(9)), (3, word(8))] {
        let mut p = picture();
        p.op(opcode, &data).op(0x28, &[0, 3, 0, 0, 1, b'A']);
        let e = render(
            &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
            &options(),
        )
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::Unsupported);
        assert!(e.message.contains("no explicit NFNT"));
    }
    let mut p = picture();
    p.op(0x28, &[0, 3, 0, 0, 1, b'C']);
    let e = render(
        &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
        &options(),
    )
    .unwrap_err();
    assert!(e.message.contains("replacement is disabled"));
}

#[test]
fn fractional_and_scaled_text_fail_instead_of_ignoring_layout_state() {
    for (op, data) in [
        (6, vec![0, 0, 0, 1]),
        (0x10, words(&[2, 2, 1, 1])),
        (5, word(0)),
    ] {
        let mut p = picture();
        p.op(op, &data).op(0x28, &[0, 3, 0, 0, 1, b'A']);
        assert_eq!(
            render(
                &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
                &options()
            )
            .unwrap_err()
            .code,
            ErrorCode::Unsupported
        );
    }
}

#[test]
fn nfnt_lengths_locations_optional_tables_and_limits_are_checked() {
    let bytes = nfnt();
    for end in 0..bytes.len() {
        assert!(
            BitmapFont::parse(&bytes[..end], &Limits::default()).is_err(),
            "prefix {end}"
        );
    }
    for (index, value) in [(16, 0xff), (32, 0xff), (34, 0xff), (47, 0)] {
        let mut data = bytes.clone();
        data[index] = value;
        assert!(BitmapFont::parse(&data, &Limits::default()).is_err());
    }
    let mut data = bytes.clone();
    data[1] = 2;
    assert_eq!(
        BitmapFont::parse(&data, &Limits::default())
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        BitmapFont::parse(
            &bytes,
            &Limits {
                max_pixels: 10,
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn cli_font_map_is_explicit_relative_and_cannot_overwrite_font_sources() {
    let dir = tempfile::tempdir().unwrap();
    let font = dir.path().join("font.nfnt");
    let map = dir.path().join("fonts.json");
    let input = dir.path().join("text.pict");
    let output = dir.path().join("text.png");
    fs::write(&font, nfnt()).unwrap();
    fs::write(
        &map,
        r#"{"schema":1,"fonts":[{"id":7,"size":3,"face":0,"path":"font.nfnt"}]}"#,
    )
    .unwrap();
    let mut p = picture();
    p.op(0x28, &[0, 3, 0, 0, 1, b'A']);
    fs::write(&input, p.finish()).unwrap();
    let run = |output: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_quickdraw"))
            .arg("convert")
            .arg("--input")
            .arg(&input)
            .arg("--output")
            .arg(output)
            .arg("--font-map")
            .arg(&map)
            .args(["--format", "png", "--overwrite"])
            .output()
            .unwrap()
    };
    let result = run(&output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.exists());
    let result = run(&font);
    assert_eq!(result.status.code(), Some(3));
    assert_eq!(fs::read(font).unwrap(), nfnt());
}
