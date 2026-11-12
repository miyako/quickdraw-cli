mod common;
use common::*;
use quickdraw::{Container, ErrorCode, Limits, RenderOptions, parse, render};

fn pixels(op: u16, data: &[u8], width: i16, height: i16) -> quickdraw::Result<Vec<u8>> {
    let mut p = Pict::new(2, width, height);
    p.op(op, data);
    let picture = parse(&p.finish(), Container::Raw, &Limits::default())?;
    Ok(render(&picture, &RenderOptions::default())?
        .image
        .rgb()
        .to_vec())
}

#[test]
fn packed_monochrome_runs_literals_and_noop() {
    let data = mono(
        8,
        [0, 0, 1, 8],
        [0, 0, 1, 8],
        0,
        &[5, 0x80, 0, 0xaa, 250, 0],
    );
    let rgb = pixels(0x98, &data, 8, 1).unwrap();
    for x in 0..8 {
        assert_eq!(
            &rgb[x * 3..x * 3 + 3],
            &[if x % 2 == 0 { 0 } else { 255 }; 3]
        );
    }
}

#[test]
fn scanline_decompression_overflow_underflow_and_truncation_fail() {
    for encoded in [
        vec![2, 248, 0], // 9 bytes into an 8-byte row
        vec![2, 250, 0], // only 7
        vec![1, 249],    // missing repeated byte
        vec![2, 7, 0],   // incomplete literal
    ] {
        let data = mono(8, [0, 0, 1, 8], [0, 0, 1, 8], 0, &encoded);
        assert_eq!(
            pixels(0x98, &data, 8, 1).unwrap_err().code,
            ErrorCode::Malformed
        );
    }
}

#[test]
fn wide_scanlines_use_word_counts_and_short_rows_are_not_packed() {
    let data = mono(
        252,
        [0, 0, 1, 8],
        [0, 0, 1, 8],
        0,
        &[0, 4, 129, 0xff, 133, 0xff],
    );
    assert_eq!(pixels(0x98, &data, 8, 1).unwrap(), vec![0; 24]);
    let data = mono(2, [0, 0, 1, 8], [0, 0, 1, 8], 0, &[0xff, 0]);
    assert_eq!(pixels(0x98, &data, 8, 1).unwrap(), vec![0; 24]);
}

#[test]
fn bitmap_region_mask_and_nearest_neighbor_destination_scaling() {
    let mut data = mono(2, [0, 0, 1, 2], [0, 0, 2, 4], 0, &[]);
    data.extend(region([0, 1, 2, 4], &[]));
    data.extend([0x80, 0]);
    let rgb = pixels(0x91, &data, 4, 2).unwrap();
    assert_eq!(&rgb[0..3], &[255; 3]);
    assert_eq!(&rgb[3..6], &[0; 3]);
    assert_eq!(&rgb[6..12], &[255; 6]);
    assert_eq!(&rgb[..12], &rgb[12..]);
}

#[test]
fn indexed_color_table_indices_and_high_byte_components() {
    for depth in [1, 2, 4, 8] {
        let mut data = pixmap_header(2, [0, 0, 1, 2], depth, 0, false, 1);
        data.extend([0; 6]);
        data.extend(1u16.to_be_bytes());
        data.extend(words(&[1, 0, -1, 0])); // index 1 green, stored first
        data.extend(words(&[0, -1, 0, 0])); // index 0 red
        data.extend(rect(0, 0, 1, 2));
        data.extend(rect(0, 0, 1, 2));
        data.extend(word(0));
        let raw = if depth == 8 {
            [0, 1]
        } else {
            [1 << (8 - depth * 2), 0]
        };
        data.extend(raw);
        assert_eq!(pixels(0x90, &data, 2, 1).unwrap(), [255, 0, 0, 0, 255, 0]);
    }
}

#[test]
fn indexed_device_table_uses_entry_order_and_missing_entries_fail() {
    let mut data = pixmap_header(2, [0, 0, 1, 1], 1, 0, false, 1);
    data.extend([0; 4]);
    data.extend([0x80, 0, 0, 0]);
    data.extend(words(&[123, -1, 0, -1]));
    data.extend(rect(0, 0, 1, 1));
    data.extend(rect(0, 0, 1, 1));
    data.extend(word(0));
    data.extend([0, 0]);
    assert_eq!(pixels(0x90, &data, 1, 1).unwrap(), [255, 0, 255]);
    let pixel_index = data.len() - 2;
    data[pixel_index] = 0x80;
    assert_eq!(
        pixels(0x90, &data, 1, 1).unwrap_err().code,
        ErrorCode::Malformed
    );
}

#[test]
fn rgb555_unpacked_and_word_packbits() {
    let rgb = pixels(
        0x9a,
        &direct(16, 1, 3, 6, &words(&[0x7c00, 0x03e0, 0x001f])),
        3,
        1,
    )
    .unwrap();
    assert_eq!(rgb, [255, 0, 0, 0, 255, 0, 0, 0, 255]);
    let packed = direct(16, 3, 4, 8, &[3, 253, 0x7c, 0]);
    assert_eq!(pixels(0x9a, &packed, 4, 1).unwrap(), [255, 0, 0].repeat(4));
}

#[test]
fn rgb32_padding_removed_padding_and_planar_packbits() {
    for (pack, data) in [
        (1, vec![42, 255, 0, 0, 99, 0, 255, 0]),
        (2, vec![255, 0, 0, 0, 255, 0]),
        (4, vec![7, 5, 255, 0, 0, 255, 0, 0]),
        (0, vec![7, 5, 255, 0, 0, 255, 0, 0]),
    ] {
        assert_eq!(
            pixels(0x9a, &direct(32, pack, 2, 8, &data), 2, 1).unwrap(),
            [255, 0, 0, 0, 255, 0]
        );
    }
}

#[test]
fn four_component_planar_data_ignores_pad_not_as_alpha() {
    let mut data = pixmap_header(8, [0, 0, 1, 2], 32, 4, true, 4);
    data.extend(rect(0, 0, 1, 2));
    data.extend(rect(0, 0, 1, 2));
    data.extend(word(0));
    data.extend([9, 7, 0, 255, 255, 0, 0, 255, 0, 0]);
    assert_eq!(pixels(0x9a, &data, 2, 1).unwrap(), [255, 0, 0, 0, 255, 0]);
}

#[test]
fn invalid_stride_palette_and_source_bounds_are_rejected() {
    let data = mono(8, [0, 0, 1, 8], [0, 0, 1, 8], 0, &[0; 8]);
    assert_eq!(
        pixels(0x90, &data, 8, 1).unwrap_err().code,
        ErrorCode::Malformed
    );
    let data = mono(2, [0, 0, 1, 17], [0, 0, 1, 17], 0, &[0, 0]);
    assert_eq!(
        pixels(0x90, &data, 17, 1).unwrap_err().code,
        ErrorCode::Malformed
    );
    let mut data = mono(2, [0, 0, 1, 2], [0, 0, 1, 2], 0, &[0, 0]);
    data[17] = 3; // source right
    assert_eq!(
        pixels(0x90, &data, 2, 1).unwrap_err().code,
        ErrorCode::Malformed
    );
}

#[test]
fn unsupported_bitmap_transfer_mode_fails_before_success() {
    let data = mono(2, [0, 0, 1, 1], [0, 0, 1, 1], 32, &[0, 0]);
    assert_eq!(
        pixels(0x90, &data, 1, 1).unwrap_err().code,
        ErrorCode::Unsupported
    );
}
