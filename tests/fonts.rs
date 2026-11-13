mod common;
use common::*;
use quickdraw::{
    BitmapFont, Container, ErrorCode, Fidelity, FontBook, Limits, RenderOptions, VectorFormat,
    export_vector, parse, render,
};
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
    data[1] = 4;
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
fn text_metadata_lengths_preserve_unknown_extensions_without_guessing() {
    for (op, known) in [(0x2d, 8), (0x2e, 4)] {
        for extra in [-1, 0, 2] {
            let length = known + extra;
            let mut data = word(length);
            data.extend(vec![0; length as usize]);
            let mut p = picture();
            p.op(op, &data).op(0x28, &[0, 3, 0, 0, 1, b'A']);
            let parsed = parse(&p.finish(), Container::Raw, &Limits::default());
            if extra < 0 {
                assert_eq!(parsed.unwrap_err().code, ErrorCode::Malformed);
            } else {
                let result = render(&parsed.unwrap(), &options());
                if extra == 0 {
                    assert!(result.is_ok());
                } else {
                    assert_eq!(result.unwrap_err().code, ErrorCode::Unsupported);
                }
            }
        }
    }
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

fn with_font(bytes: &[u8]) -> RenderOptions {
    let mut result = options();
    result.fonts = FontBook::default();
    result
        .fonts
        .insert(
            7,
            3,
            0,
            BitmapFont::parse(bytes, &Limits::default()).unwrap(),
            &Limits::default(),
        )
        .unwrap();
    result.fidelity = Fidelity::Approximate;
    result
}

fn optional_font(flags: u8) -> Vec<u8> {
    let mut bytes = nfnt();
    bytes[1] = flags;
    if flags & 2 != 0 {
        bytes.extend(words(&[0x0280, 0x0380, 0x0200]));
    }
    if flags & 1 != 0 {
        bytes.extend(words(&[3, 3, 3]));
    }
    bytes
}

#[test]
fn optional_nfnt_tables_have_glyph_entries_not_location_terminators() {
    for flags in 1..=3 {
        let font = optional_font(flags);
        assert!(BitmapFont::parse(&font, &Limits::default()).is_ok());
        for end in 48..font.len() {
            assert_eq!(
                BitmapFont::parse(&font[..end], &Limits::default())
                    .unwrap_err()
                    .code,
                ErrorCode::Malformed
            );
        }
        let mut extra = font;
        extra.extend(word(-1));
        assert_eq!(
            BitmapFont::parse(&extra, &Limits::default())
                .unwrap_err()
                .code,
            ErrorCode::Malformed
        );
    }
    for height in [0x0202, 0x0102] {
        let mut font = optional_font(1);
        font[48..50].copy_from_slice(&word(height));
        assert_eq!(
            BitmapFont::parse(&font, &Limits::default())
                .unwrap_err()
                .code,
            ErrorCode::Malformed
        );
    }
    let mut font = optional_font(3);
    font[26] = 0;
    font[54..60].copy_from_slice(&words(&[0x0102; 3]));
    let mut p = picture();
    p.op(0x28, &[0, 3, 0, 0, 1, b'A']);
    let image = render(
        &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
        &with_font(&font),
    )
    .unwrap()
    .image;
    assert_eq!(at(image.rgb(), 1, 0), &[255; 3]);
    assert_eq!(at(image.rgb(), 0, 1), &[0; 3]);
}

#[test]
fn fractional_nfnt_advances_are_enabled_explicitly_and_accumulate_before_rounding() {
    for (enabled, fraction, expected, pen) in [
        (0, 0x8000u16, vec![1, 5, 9], 12),
        (1, 0x8000, vec![1, 4, 6], 8),
        (1, 0x4000, vec![1, 3, 6], 7),
    ] {
        let mut p = picture();
        p.op(0x2e, &[0, 4, 0, 1, enabled, 1])
            .op(0x15, &fraction.to_be_bytes())
            .op(0x28, &[0, 3, 0, 0, 3, b'A', b'A', b'A'])
            .op(0x21, &words(&[3, pen]));
        let r = render(
            &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
            &with_font(&optional_font(3)),
        )
        .unwrap();
        let actual: Vec<_> = (0..12)
            .filter(|&x| at(r.image.rgb(), x, 0) == [0; 3])
            .collect();
        assert_eq!(actual, expected);
        if pen < 12 {
            assert_eq!(at(r.image.rgb(), pen as usize, 3), &[0; 3]);
        }
    }
}

#[test]
fn fractional_space_extra_is_signed_and_does_not_change_nonspace_widths() {
    let mut font = words(&[0x1000, 32, 33, 2, 0, 0, 1, 3, 12, 3, 0, 0, 1]);
    font.extend([0xc0, 0, 0xc0, 0, 0xc0, 0]);
    font.extend(words(&[0, 0, 1, 2, 2, 1, 1, -1]));
    for (extra, ratio, expected) in [
        (32768i32, 1, vec![3, 6, 10]),
        (-32768, 1, vec![2, 4, 7]),
        (49152, 2, vec![6, 7]),
    ] {
        let mut p = picture();
        p.op(6, &extra.to_be_bytes())
            .op(0x10, &words(&[1, ratio, 1, 1]))
            .op(0x28, &[0, 3, 0, 0, 6, b' ', b'!', b' ', b'!', b' ', b'!']);
        let r = render(
            &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
            &with_font(&font),
        )
        .unwrap();
        let actual: Vec<_> = (0..12)
            .filter(|&x| at(r.image.rgb(), x, 0) == [0; 3])
            .collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn ratios_scale_glyphs_bearings_and_pen_with_origin_and_clip() {
    let mut p = picture();
    p.op(0x10, &words(&[2, 3, 1, 2]))
        .op(0x0c, &words(&[2, 1]))
        .op(1, &region([1, 4, 7, 6], &[]))
        .op(0x28, &[0, 7, 0, 2, 1, b'A'])
        .op(1, &region([1, 2, 9, 14], &[]))
        .op(0x21, &words(&[7, 10]));
    let r = render(
        &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
        &with_font(&nfnt()),
    )
    .unwrap();
    for y in [0, 1] {
        assert_eq!(at(r.image.rgb(), 2, y), &[0; 3]);
        assert_eq!(at(r.image.rgb(), 3, y), &[0; 3]);
        assert_eq!(at(r.image.rgb(), 4, y), &[255; 3]);
    }
    assert_eq!(at(r.image.rgb(), 6, 6), &[0; 3]);
    assert_eq!(at(r.image.rgb(), 5, 6), &[255; 3]);
}

#[test]
fn all_basic_text_modes_apply_both_ink_and_background_in_a_single_run() {
    let expected = [
        (0, 255),
        (0, 85),
        (170, 85),
        (255, 85),
        (255, 0),
        (85, 0),
        (85, 170),
        (85, 255),
    ];
    for (mode, (ink, blank)) in expected.into_iter().enumerate() {
        let mut p = picture();
        p.op(5, &word(mode as i16)).op(0x28, &[0, 3, 0, 0, 1, b'A']);
        let mut options = with_font(&nfnt());
        options.background = [85; 3];
        let r = render(
            &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
            &options,
        )
        .unwrap();
        assert_eq!(at(r.image.rgb(), 1, 0), &[ink; 3], "mode {mode}");
        assert_eq!(at(r.image.rgb(), 0, 0), &[blank; 3], "mode {mode}");
        assert_eq!(
            at(r.image.rgb(), 3, 0),
            &[blank; 3],
            "advance cell, mode {mode}"
        );
        assert_eq!(
            at(r.image.rgb(), 4, 0),
            &[85; 3],
            "outside run, mode {mode}"
        );
    }
}

#[test]
fn overlapping_glyphs_form_one_mask_before_xor_and_arithmetic_compositing() {
    let mut p = picture();
    p.op(5, &word(2))
        .op(0x16, &word(-4))
        .op(0x28, &[0, 3, 0, 0, 2, b'A', b'A']);
    let mut options = with_font(&nfnt());
    options.background = [85; 3];
    let r = render(
        &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
        &options,
    )
    .unwrap();
    assert_eq!(at(r.image.rgb(), 1, 0), &[170; 3]);
    for mode in [32, 32 + 64, 36] {
        let mut p = picture();
        p.op(0x1a, &words(&[-1, 0, 0]))
            .op(0x1b, &words(&[0, 0, -1]))
            .op(0x1f, &words(&[-1; 3]))
            .op(5, &word(mode))
            .op(0x28, &[0, 3, 0, 0, 1, b'A']);
        let r = render(
            &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
            &options,
        )
        .unwrap();
        assert_eq!(at(r.image.rgb(), 1, 0), &[255, 0, 0]);
        assert_eq!(
            at(r.image.rgb(), 0, 0),
            if mode == 32 { &[0, 0, 255] } else { &[85; 3] }
        );
    }
}

#[test]
fn text_mask_clips_right_overhang_but_does_not_paint_background() {
    let mut font = nfnt();
    font[40..42].copy_from_slice(&word(1));
    let mut options = with_font(&font);
    options.background = [85; 3];
    let mut p = picture();
    p.op(5, &word(64)).op(0x28, &[0, 3, 0, 0, 1, b'A']);
    let r = render(
        &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
        &options,
    )
    .unwrap();
    assert_eq!(at(r.image.rgb(), 1, 0), &[85; 3]);
    assert_eq!(at(r.image.rgb(), 0, 0), &[85; 3]);
    assert_eq!(at(r.image.rgb(), 0, 1), &[0; 3]);
}

#[test]
fn scaled_text_limits_and_unsupported_layout_do_not_panic_or_substitute() {
    for (op, data, expected) in [
        (0x10, words(&[1, 1, 0, 1]), ErrorCode::Malformed),
        (0x10, words(&[-1, 1, 1, 1]), ErrorCode::Unsupported),
        (0x2e, vec![0, 4, 1, 0, 0, 0], ErrorCode::Unsupported),
        (0x2e, vec![0, 4, 0, 0, 2, 0], ErrorCode::Malformed),
        (5, word(49), ErrorCode::Unsupported),
        (0x10, words(&[32767, 32767, 1, 1]), ErrorCode::ResourceLimit),
    ] {
        let mut p = picture();
        p.op(op, &data).op(0x28, &[0, 3, 0, 0, 1, b'A']);
        let result = parse(&p.finish(), Container::Raw, &Limits::default())
            .and_then(|p| render(&p, &with_font(&nfnt())));
        assert_eq!(result.unwrap_err().code, expected);
    }
}

#[test]
fn vector_text_keeps_source_only_modes_as_paths_and_rejects_destination_modes() {
    for mode in [0, 4, 5, 7, 36, 64] {
        let mut p = picture();
        p.op(5, &word(mode))
            .op(0x10, &words(&[1, 2, 1, 1]))
            .op(0x28, &[0, 3, 0, 0, 1, b'A']);
        let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
        for format in [VectorFormat::Svg, VectorFormat::Pdf] {
            let r = export_vector(&pic, &with_font(&nfnt()), 72.0, format).unwrap();
            let text = String::from_utf8_lossy(&r.bytes);
            assert!(!text.contains("/Subtype /Image") && !text.contains("<image"));
        }
    }
    let mut p = picture();
    p.op(5, &word(2)).op(0x28, &[0, 3, 0, 0, 1, b'A']);
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    assert_eq!(
        export_vector(&pic, &with_font(&nfnt()), 72.0, VectorFormat::Pdf)
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}

#[test]
fn shrinking_and_negative_positions_keep_fixed_point_pen_rounding() {
    let mut p = picture();
    p.op(0x10, &words(&[2, 2, 3, 3]))
        .op(0x28, &[0, 3, 255, 255, 1, b'A'])
        .op(0x21, &words(&[3, 4]));
    let r = render(
        &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
        &with_font(&nfnt()),
    )
    .unwrap();
    assert_eq!(at(r.image.rgb(), 0, 1), &[0; 3]);
    assert_eq!(at(r.image.rgb(), 0, 2), &[255; 3]);
    assert_eq!(at(r.image.rgb(), 2, 3), &[0; 3]);
    assert_eq!(at(r.image.rgb(), 1, 3), &[255; 3]);
}

#[test]
fn disabled_scaling_and_strict_fractional_layout_are_not_silently_accepted() {
    let mut p = picture();
    p.op(0x2e, &[0, 4, 0, 0, 0, 1])
        .op(0x10, &words(&[2, 2, 1, 1]))
        .op(0x28, &[0, 3, 0, 0, 1, b'A']);
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let error = render(&pic, &with_font(&nfnt())).unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(error.message.contains("disabled"));
    let mut p = picture();
    p.op(0x2e, &[0, 4, 0, 0, 1, 0])
        .op(0x28, &[0, 3, 0, 0, 1, b'A']);
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let mut opts = with_font(&optional_font(2));
    opts.fidelity = Fidelity::Strict;
    assert_eq!(
        render(&pic, &opts).unwrap_err().code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        export_vector(&pic, &opts, 72.0, VectorFormat::Svg)
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}
