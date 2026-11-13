mod common;
use common::*;
use quickdraw::{
    BitmapFont, Container, ErrorCode, Fidelity, FontBook, Limits, OutlineEncoding, OutlineFont,
    OutlineSelection, PrinterComments, RenderOptions, VectorFormat, export_vector, parse, render,
};

fn options(outline: bool) -> RenderOptions {
    let limits = Limits::default();
    let mut fonts = FontBook::default();
    if outline {
        fonts
            .insert_outline(
                7,
                10,
                0,
                OutlineFont::parse(
                    include_bytes!("fixtures/portable-outline.ttf"),
                    OutlineSelection {
                        face_index: 0,
                        encoding: OutlineEncoding::Macroman,
                        kerning: false,
                    },
                    &limits,
                )
                .unwrap(),
                &limits,
            )
            .unwrap();
    } else {
        fonts
            .insert(
                7,
                3,
                0,
                BitmapFont::parse(&nfnt(), &limits).unwrap(),
                &limits,
            )
            .unwrap();
    }
    RenderOptions {
        fidelity: Fidelity::Approximate,
        printer_comments: PrinterComments::ApplePortable,
        fonts,
        ..Default::default()
    }
}
fn comment(p: &mut Pict, kind: i16, data: &[u8]) {
    let mut body = words(&[kind, data.len() as i16]);
    body.extend(data);
    p.op(0xa1, &body);
}
fn begin(p: &mut Pict, angle: i16) {
    comment(p, 150, &words(&[0, angle, 0]));
    comment(p, 154, &[0; 8]);
}
fn text(p: &mut Pict, x: i16, y: i16) {
    let mut body = words(&[y, x]);
    body.extend([1, b'A']);
    p.op(0x28, &body);
}
fn picture(outline: bool) -> Pict {
    let mut p = Pict::new(2, 64, 64);
    p.op(3, &word(7))
        .op(0x0d, &word(if outline { 10 } else { 3 }))
        .op(0x15, &word(0));
    p
}
fn pixels(p: Pict, opts: &RenderOptions) -> Vec<u8> {
    let pic = parse(&p.finish(), Container::Raw, &opts.limits).unwrap();
    render(&pic, opts).unwrap().image.rgb().to_vec()
}
fn code(p: Pict, opts: &RenderOptions) -> ErrorCode {
    let pic = parse(&p.finish(), Container::Raw, &opts.limits).unwrap();
    let a = render(&pic, opts).unwrap_err().code;
    assert_eq!(
        export_vector(&pic, opts, 72., VectorFormat::Svg)
            .unwrap_err()
            .code,
        a
    );
    a
}

#[test]
fn clockwise_text_uses_first_baseline_ignores_clip_and_suppresses_only_fallback() {
    for outline in [false, true] {
        let opts = options(outline);
        let mut plain = picture(outline);
        text(&mut plain, 16, 16);
        text(&mut plain, 24, 16);
        let source = pixels(plain, &opts);
        let mut rotated = picture(outline);
        rotated.op(0x0c, &words(&[3, 2]));
        comment(&mut rotated, 190, &[]);
        comment(&mut rotated, 191, &[]);
        begin(&mut rotated, 90);
        rotated.op(1, &region([0, 0, 0, 0], &[]));
        text(&mut rotated, 19, 18);
        text(&mut rotated, 27, 18);
        rotated.op(0x90, &mono(2, [0, 0, 1, 1], [40, 40, 42, 42], 0, &[128, 0]));
        comment(&mut rotated, 151, &[]);
        // The ignored-in-group clip still governs ordinary drawing after TextEnd.
        rotated.op(0x31, &rect(0, 0, 64, 64));
        let bytes = rotated.finish();
        let pic = parse(&bytes, Container::Raw, &opts.limits).unwrap();
        let actual = render(&pic, &opts).unwrap().image.rgb().to_vec();
        let mut expected = vec![255; 64 * 64 * 3];
        for y in 0..32 {
            for x in 0..64 {
                let src = (y * 64 + x) * 3;
                let dst = (x * 64 + 31 - y) * 3;
                expected[dst..dst + 3].copy_from_slice(&source[src..src + 3]);
            }
        }
        assert_eq!(actual, expected);
        let svg = String::from_utf8(
            export_vector(&pic, &opts, 72., VectorFormat::Svg)
                .unwrap()
                .bytes,
        )
        .unwrap();
        assert!(!svg.contains("<image"));
        let ignored = RenderOptions {
            printer_comments: PrinterComments::Ignore,
            ..opts
        };
        assert_ne!(render(&pic, &ignored).unwrap().image.rgb(), actual);
    }
}

#[test]
fn text_center_reanchors_fixed_angle_takes_precedence_and_text_end_resets() {
    let opts = options(false);
    let mut first = picture(false);
    begin(&mut first, 90);
    text(&mut first, 16, 16);
    comment(&mut first, 154, &[0; 8]);
    text(&mut first, 24, 16);
    comment(&mut first, 151, &[]);
    text(&mut first, 40, 16);
    let expected = pixels(first, &opts);
    let mut p = picture(false);
    let mut data = words(&[0, -90, 0]);
    data.extend((90i32 * 65536).to_be_bytes());
    comment(&mut p, 150, &data);
    comment(&mut p, 154, &[0; 8]);
    text(&mut p, 16, 16);
    comment(&mut p, 151, &[]);
    begin(&mut p, 90);
    text(&mut p, 24, 16);
    comment(&mut p, 151, &[]);
    text(&mut p, 40, 16);
    assert_eq!(pixels(p, &opts), expected);
}

#[test]
fn fractional_rotation_center_and_axis_flips_match_inverse_pixel_sampling() {
    let opts = options(false);
    let mut plain = picture(false);
    text(&mut plain, 16, 16);
    let source = pixels(plain, &opts);
    for (angle, flip) in [(33.5f64, 0u8), (0.0, 1), (0.0, 2)] {
        let mut p = picture(false);
        let mut body = vec![0, flip, 0, 0, 0, 0];
        body.extend(((angle * 65536.0) as i32).to_be_bytes());
        comment(&mut p, 150, &body);
        comment(
            &mut p,
            154,
            &[(-8192i32).to_be_bytes(), 8192i32.to_be_bytes()].concat(),
        );
        text(&mut p, 16, 16);
        comment(&mut p, 151, &[]);
        let actual = pixels(p, &opts);
        let (s, c) = angle.to_radians().sin_cos();
        let mut expected = vec![255; 64 * 64 * 3];
        for y in 0..64 {
            for x in 0..64 {
                let (dx, dy) = (x as f64 + 0.5 - 16.125, y as f64 + 0.5 - 15.875);
                let sx = (16.125 + (c * dx + s * dy) * if flip == 1 { -1.0 } else { 1.0 }).floor()
                    as i32;
                let sy = (15.875 + (-s * dx + c * dy) * if flip == 2 { -1.0 } else { 1.0 }).floor()
                    as i32;
                if (0..64).contains(&sx) && (0..64).contains(&sy) {
                    let src = (sy as usize * 64 + sx as usize) * 3;
                    let dst = (y * 64 + x) * 3;
                    expected[dst..dst + 3].copy_from_slice(&source[src..src + 3]);
                }
            }
        }
        assert!(
            actual == expected,
            "{angle}/{flip}: first differing byte {:?}",
            actual.iter().zip(&expected).position(|(a, b)| a != b)
        );
    }
}

#[test]
fn fractional_widths_multiply_survive_pen_size_and_reset_with_inverse() {
    let opts = options(false);
    let mut p = picture(false);
    comment(&mut p, 182, &words(&[1, 4]));
    p.op(7, &words(&[8, 8]));
    p.op(0x20, &words(&[2, 1, 2, 10]));
    comment(&mut p, 182, &words(&[2, 1]));
    p.op(7, &words(&[4, 4]));
    p.op(0x20, &words(&[6, 1, 6, 10]));
    comment(&mut p, 182, &words(&[2, 1]));
    p.op(7, &words(&[1, 1]));
    p.op(0x20, &words(&[10, 1, 10, 10]));
    let got = pixels(p, &opts);
    let mut reference = picture(false);
    reference.op(7, &words(&[2, 2]));
    reference.op(0x20, &words(&[2, 1, 2, 10]));
    reference.op(0x20, &words(&[6, 1, 6, 10]));
    reference.op(7, &words(&[1, 1]));
    reference.op(0x20, &words(&[10, 1, 10, 10]));
    assert_eq!(got, pixels(reference, &opts));
    let mut fractional = picture(false);
    comment(&mut fractional, 182, &words(&[3, 4]));
    fractional.op(0x20, &words(&[2, 1, 2, 10]));
    let pic = parse(&fractional.finish(), Container::Raw, &opts.limits).unwrap();
    let svg = String::from_utf8(
        export_vector(&pic, &opts, 72., VectorFormat::Svg)
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert!(svg.contains("10.75000000 2.75000000"));
    assert_eq!(
        &render(&pic, &opts).unwrap().image.rgb()[(2 * 64 + 10) * 3..(2 * 64 + 11) * 3],
        &[0; 3]
    );
}

#[test]
fn printer_groups_variants_unknown_semantics_and_limits_fail_explicitly() {
    let opts = options(false);
    for (kind, data, expected) in [
        (150, vec![0; 5], ErrorCode::Malformed),
        (150, vec![1, 0, 0, 90, 0, 0], ErrorCode::Unsupported),
        (151, vec![], ErrorCode::Malformed),
        (154, vec![0; 8], ErrorCode::Malformed),
        (182, words(&[1, 0]), ErrorCode::Unsupported),
        (
            192,
            b"PostScript is data, never executed".to_vec(),
            ErrorCode::Unsupported,
        ),
        (155, vec![], ErrorCode::Unsupported),
    ] {
        let mut p = picture(false);
        comment(&mut p, kind, &data);
        assert_eq!(code(p, &opts), expected);
    }
    let mut p = picture(false);
    begin(&mut p, 90);
    assert_eq!(code(p, &opts), ErrorCode::Malformed);
    let mut p = picture(false);
    comment(&mut p, 150, &words(&[0, 90, 0]));
    text(&mut p, 16, 16);
    assert_eq!(code(p, &opts), ErrorCode::Malformed);
    let mut p = picture(false);
    begin(&mut p, 90);
    p.op(0x0c, &words(&[1, 0]));
    assert_eq!(code(p, &opts), ErrorCode::Unsupported);
    let mut p = picture(false);
    comment(&mut p, 190, &[]);
    p.op(0x31, &rect(0, 0, 2, 2));
    assert_eq!(code(p, &opts), ErrorCode::Unsupported);
    for op in [0x20, 0x30] {
        let mut p = picture(false);
        comment(&mut p, 182, &words(&[1, 2]));
        p.op(op, &words(&[1, 1, 4, 4]));
        assert_eq!(code(p, &opts), ErrorCode::Unsupported);
    }
    let mut p = picture(false);
    for _ in 0..20 {
        comment(&mut p, 182, &words(&[32767, 1]));
    }
    assert_eq!(code(p, &opts), ErrorCode::ResourceLimit);
    let mut limited = options(false);
    limited.limits.max_operations = 10;
    let mut p = picture(false);
    begin(&mut p, 33);
    text(&mut p, 16, 16);
    comment(&mut p, 151, &[]);
    assert_eq!(code(p, &limited), ErrorCode::ResourceLimit);
    for mode in [0, 65] {
        let mut p = picture(false);
        p.op(5, &word(mode));
        begin(&mut p, 90);
        text(&mut p, 16, 16);
        comment(&mut p, 151, &[]);
        assert_eq!(code(p, &opts), ErrorCode::Unsupported);
    }
    let mut p = picture(false);
    begin(&mut p, 90);
    text(&mut p, 16, 16);
    comment(&mut p, 151, &[]);
    let strict = RenderOptions {
        fidelity: Fidelity::Strict,
        ..opts
    };
    assert_eq!(code(p, &strict), ErrorCode::Unsupported);
}
