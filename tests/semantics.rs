mod common;
use common::*;
use quickdraw::{Container, ErrorCode, Fidelity, Limits, RenderOptions, parse, render};

fn pixels(p: Pict, approximate: bool) -> Vec<u8> {
    let p = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    render(
        &p,
        &RenderOptions {
            fidelity: if approximate {
                Fidelity::Approximate
            } else {
                Fidelity::Strict
            },
            ..Default::default()
        },
    )
    .unwrap()
    .image
    .rgb()
    .to_vec()
}
fn at(data: &[u8], width: usize, x: usize, y: usize) -> &[u8] {
    &data[(y * width + x) * 3..(y * width + x) * 3 + 3]
}
fn color(rgb: [u8; 3]) -> Vec<u8> {
    rgb.into_iter()
        .flat_map(|v| (u16::from(v) * 257).to_be_bytes())
        .collect()
}

#[test]
fn origin_delta_moves_coordinates_and_clip_not_existing_pixels() {
    let mut p = Pict::new(2, 6, 4);
    p.op(1, &region([0, 1, 4, 5], &[]));
    p.op(0x31, &rect(0, 4, 1, 5));
    p.op(0x0c, &words(&[2, 1])).op(0x31, &rect(1, 2, 2, 3));
    p.op(0x0c, &words(&[-1, 0])).op(0x31, &rect(3, 1, 4, 2));
    let data = pixels(p, false);
    assert_eq!(at(&data, 6, 4, 0), &[0; 3]); // prior pixels did not move
    assert_eq!(at(&data, 6, 0, 0), &[0; 3]); // unchanged logical clip moved on screen
    assert_eq!(at(&data, 6, 0, 2), &[0; 3]); // delta is cumulative, not absolute
    assert_eq!(at(&data, 6, 1, 0), &[255; 3]);
}

#[test]
fn origin_preserves_logical_pen_and_last_rectangle_and_pattern_phase() {
    let mut p = Pict::new(2, 8, 4);
    p.op(0x20, &words(&[2, 4, 2, 4]));
    p.op(0x32, &rect(2, 2, 3, 4));
    p.op(0x0c, &words(&[2, 1]));
    p.op(0x21, &words(&[2, 5])); // same logical pen, physically shifted
    p.op(0x0a, &[0xaa; 8]).op(0x3c, &[]);
    let data = pixels(p, false);
    assert_eq!(at(&data, 8, 4, 2), &[0; 3]);
    assert_eq!(at(&data, 8, 2, 1), &[0; 3]);
    assert_eq!(at(&data, 8, 3, 1), &[0; 3]);
    assert_eq!(at(&data, 8, 0, 1), &[0; 3]);
    assert_eq!(at(&data, 8, 1, 1), &[255; 3]);
}

#[test]
fn origin_shifts_destination_and_mask_without_changing_bitmap_source() {
    let mut p = Pict::new(2, 4, 1);
    p.op(0x0c, &words(&[2, 0]));
    let mut bits = mono(2, [10, 20, 11, 22], [0, 2, 1, 6], 0, &[]);
    bits.extend(region([0, 3, 1, 6], &[]));
    bits.extend([0x80, 0]);
    p.op(0x91, &bits);
    let data = pixels(p, false);
    assert_eq!(data, [255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255]);
}

#[test]
fn rounded_rectangle_clamps_corner_diameters_and_same_shape_reads_new_oval_size() {
    let mut p = Pict::new(2, 8, 4);
    p.op(0x0b, &words(&[20, 20])).op(0x41, &rect(0, 0, 4, 4));
    p.op(0x0b, &words(&[0, 0]))
        .op(0x0c, &words(&[-4, 0]))
        .op(0x49, &[]);
    let data = pixels(p, true);
    assert_eq!(at(&data, 8, 0, 0), &[255; 3]);
    assert_eq!(at(&data, 8, 1, 1), &[0; 3]);
    assert_eq!(at(&data, 8, 4, 0), &[0; 3]);
}

#[test]
fn rounded_rectangle_frame_stays_inside_and_does_not_paint_interior() {
    let mut p = Pict::new(2, 8, 6);
    p.op(0x0b, &words(&[4, 4]))
        .op(7, &words(&[1, 2]))
        .op(0x40, &rect(0, 0, 6, 8));
    let data = pixels(p, true);
    assert_eq!(at(&data, 8, 0, 0), &[255; 3]);
    assert_eq!(at(&data, 8, 1, 2), &[0; 3]);
    assert_eq!(at(&data, 8, 3, 2), &[255; 3]);
}

#[test]
fn arc_angles_are_clockwise_from_twelve_and_scaled_to_the_oval() {
    for (start, sweep, lit, clear) in [
        (0, 90, (5, 1), (2, 1)),
        (0, -90, (2, 1), (5, 1)),
        (270, 180, (5, 1), (5, 2)),
        (360, 45, (4, 0), (6, 1)),
    ] {
        let mut p = Pict::new(2, 8, 4);
        let mut arc = rect(0, 0, 4, 8);
        arc.extend(words(&[start, sweep]));
        p.op(0x61, &arc);
        let data = pixels(p, true);
        assert_eq!(at(&data, 8, lit.0, lit.1), &[0; 3], "{start}/{sweep}");
        assert_eq!(at(&data, 8, clear.0, clear.1), &[255; 3], "{start}/{sweep}");
    }
}

#[test]
fn zero_full_and_same_arc_and_frame_radial_edges() {
    for (sweep, expected) in [(0, 255), (360, 0), (-720, 0)] {
        let mut p = Pict::new(2, 8, 8);
        p.op(0x32, &rect(0, 0, 8, 8)).op(0x69, &words(&[20, sweep]));
        assert_eq!(at(&pixels(p, true), 8, 4, 4), &[expected; 3]);
    }
    let mut p = Pict::new(2, 8, 8);
    let mut data = rect(0, 0, 8, 8);
    data.extend(words(&[0, 90]));
    p.op(0x60, &data);
    let data = pixels(p, true);
    assert_eq!(at(&data, 8, 4, 0), &[0; 3]);
    assert_eq!(at(&data, 8, 4, 2), &[255; 3]); // FrameArc does not frame radii
}

#[test]
fn color_pixel_patterns_ignore_modes_and_foreground_but_bit_patterns_restore_them() {
    let mut p = Pict::new(2, 4, 2);
    p.op(0x1a, &color([0, 0, 255])).op(8, &word(10));
    let mut solid = word(2);
    solid.extend([0; 8]);
    solid.extend(color([255, 0, 0]));
    p.op(0x13, &solid)
        .op(0x31, &rect(0, 0, 2, 4))
        .op(0x31, &rect(0, 0, 2, 4));
    p.op(9, &[255; 8])
        .op(8, &word(8))
        .op(0x31, &rect(0, 0, 1, 1));
    let data = pixels(p, false);
    assert_eq!(at(&data, 4, 1, 1), &[255, 0, 0]);
    assert_eq!(at(&data, 4, 0, 0), &[0, 0, 255]);
}

#[test]
fn indexed_pixel_pattern_repeats_after_origin_changes() {
    let mut tile = word(1);
    tile.extend([0; 8]);
    tile.extend(pixmap_header(2, [0, 0, 2, 2], 1, 0, false, 1));
    tile[20..22].copy_from_slice(&word(4));
    tile.extend([0; 6]);
    tile.extend(word(1));
    tile.extend(word(0));
    tile.extend(color([255, 0, 0]));
    tile.extend(word(1));
    tile.extend(color([0, 255, 0]));
    tile.extend([0x40, 0, 0x80, 0]);
    let mut p = Pict::new(2, 4, 2);
    p.op(0x14, &tile)
        .op(0x0c, &words(&[1, 0]))
        .op(0x34, &rect(0, 1, 2, 5));
    let data = pixels(p, false);
    assert_eq!(&data[..6], &[255, 0, 0, 0, 255, 0]);
    assert_eq!(&data[12..18], &[0, 255, 0, 255, 0, 0]);
}

#[test]
fn all_arithmetic_modes_have_documented_component_behavior() {
    let source = [200, 80, 10];
    let dest = [100, 40, 220];
    for (mode, expected) in [
        (32, [150, 60, 114]),
        (33, [128, 120, 128]),
        (34, [44, 120, 230]),
        (35, [128, 128, 210]),
        (36, source),
        (37, [200, 80, 220]),
        (38, [156, 216, 210]),
        (39, [100, 40, 10]),
    ] {
        let mut p = Pict::new(2, 1, 1);
        p.op(0x1a, &color(dest)).op(0x31, &rect(0, 0, 1, 1));
        p.op(0x1f, &words(&[-32768; 3]));
        let mut bitmap = direct(32, 1, 1, 4, &[0, source[0], source[1], source[2]]);
        let i = bitmap.len() - 6;
        bitmap[i..i + 2].copy_from_slice(&(mode as u16).to_be_bytes());
        p.op(0x9a, &bitmap);
        assert_eq!(pixels(p, false), expected, "mode {mode}");
    }
}

#[test]
fn opcolor_weights_extremes_pinning_and_missing_state_are_explicit() {
    let mut p = Pict::new(2, 1, 1);
    p.op(0x1f, &words(&[0, -1, -32768]))
        .op(8, &word(32))
        .op(0x31, &rect(0, 0, 1, 1));
    assert_eq!(pixels(p, false), [255, 0, 127]);
    let mut p = Pict::new(2, 1, 1);
    p.op(8, &word(32)).op(0x31, &rect(0, 0, 1, 1));
    let p = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    assert!(
        render(&p, &RenderOptions::default())
            .unwrap_err()
            .message
            .contains("OpColor")
    );
}

#[test]
fn transparent_skips_the_whole_background_color_not_individual_channels() {
    let mut p = Pict::new(2, 2, 1);
    p.op(0x1b, &color([255, 0, 0]));
    let mut bitmap = direct(32, 1, 2, 8, &[0, 255, 0, 0, 0, 255, 0, 10]);
    let i = bitmap.len() - 10;
    bitmap[i..i + 2].copy_from_slice(&36u16.to_be_bytes());
    p.op(0x9a, &bitmap);
    assert_eq!(pixels(p, false), [255, 255, 255, 255, 0, 10]);
}

#[test]
fn highlighting_exchanges_only_background_and_highlight_and_resets_after_drawing() {
    let mut p = Pict::new(2, 3, 1);
    p.op(0x1d, &color([255, 255, 0]));
    p.op(0x31, &rect(0, 0, 1, 1)); // black preserved by highlight
    p.op(0x1c, &[]).op(0x33, &rect(0, 0, 1, 3));
    p.op(0x33, &rect(0, 2, 1, 3)); // flag reset: ordinary inversion
    p.op(0x1c, &[]).op(0x33, &rect(0, 1, 1, 2)); // deselection
    assert_eq!(pixels(p, false), [0, 0, 0, 255, 255, 255, 0, 0, 255]);
}

#[test]
fn highlight_pattern_mask_and_explicit_mode_preserve_unselected_pixels() {
    let mut p = Pict::new(2, 2, 1);
    p.op(0x1d, &color([255, 255, 0]))
        .op(9, &[0x80; 8])
        .op(8, &word(58));
    p.op(0x31, &rect(0, 0, 1, 2));
    assert_eq!(pixels(p, false), [255, 255, 0, 255, 255, 255]);
}

#[test]
fn unknown_system_highlight_color_and_dither_modes_never_silently_fallback() {
    let mut p = Pict::new(2, 1, 1);
    p.op(0x1e, &[]).op(0x1c, &[]).op(0x33, &rect(0, 0, 1, 1));
    let p = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    assert_eq!(
        render(&p, &RenderOptions::default()).unwrap_err().code,
        ErrorCode::Unsupported
    );
    let image = render(
        &p,
        &RenderOptions {
            highlight: Some([0, 255, 255]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(image.image.rgb(), &[0, 255, 255]);
}

#[test]
fn rgb_copy_applies_foreground_background_coloration() {
    let mut p = Pict::new(2, 1, 1);
    p.op(0x1a, &color([0, 0, 255]))
        .op(0x1b, &color([255, 0, 0]));
    p.op(0x9a, &direct(32, 1, 1, 4, &[0, 128, 128, 128]));
    assert_eq!(pixels(p, false), [128, 0, 127]);
}
