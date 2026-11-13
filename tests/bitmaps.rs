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

#[test]
fn independently_packed_rows_cover_versions_palettes_components_and_prefix_boundary() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/pixmaps.json")).unwrap();
    let bytes = |v: &serde_json::Value| {
        let s = v.as_str().unwrap();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>()
    };
    for case in fixtures["cases"].as_array().unwrap() {
        let packet = bytes(&case["packet"]);
        assert_eq!(
            pixels(
                case["opcode"].as_u64().unwrap() as u16,
                &packet,
                case["width"].as_i64().unwrap() as i16,
                2
            )
            .unwrap(),
            bytes(&case["rgb"]),
            "{}",
            case["name"]
        );
        let mut p = Pict::new(2, case["width"].as_i64().unwrap() as i16, 2);
        p.op(case["opcode"].as_u64().unwrap() as u16, &packet);
        let stream = p.finish();
        for limits in [
            Limits {
                max_decoded_bytes: 1,
                ..Limits::default()
            },
            Limits {
                max_work: stream.len() as u64,
                ..Limits::default()
            },
        ] {
            assert_eq!(
                parse(&stream, Container::Raw, &limits).unwrap_err().code,
                ErrorCode::ResourceLimit
            );
        }
        for cut in 0..stream.len() {
            assert!(
                parse(&stream[..cut], Container::Raw, &Limits::default()).is_err(),
                "{} cut {cut}",
                case["name"]
            );
        }
    }
}

#[test]
fn pixmap_private_flags_extensions_and_reserved_packing_are_not_discarded() {
    let base = direct(32, 1, 2, 8, &[0, 255, 0, 0, 0, 0, 255, 0]);
    for (offset, value, expected) in [
        (14, 1u32, ErrorCode::Unsupported),
        (18, 1, ErrorCode::Malformed),
        (38, 1, ErrorCode::Unsupported),
        (46, 1, ErrorCode::Unsupported),
    ] {
        let mut data = base.clone();
        if offset == 14 {
            data[offset..offset + 2].copy_from_slice(&(value as u16).to_be_bytes());
        } else {
            data[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        assert_eq!(pixels(0x9a, &data, 2, 1).unwrap_err().code, expected);
    }
    let mut indexed = pixmap_header(2, [0, 0, 1, 1], 1, 0, false, 1);
    indexed.extend([0; 4]);
    indexed.extend(words(&[0x4000, 0, 0, -1, -1, -1]));
    indexed.extend(rect(0, 0, 1, 1));
    indexed.extend(rect(0, 0, 1, 1));
    indexed.extend(word(0));
    indexed.extend([0, 0]);
    assert_eq!(
        pixels(0x90, &indexed, 1, 1).unwrap_err().code,
        ErrorCode::Unsupported
    );
}

#[test]
fn inactive_dither_flag_preserves_all_source_modes_clip_scale_and_overlap() {
    use quickdraw::{Fidelity, VectorFormat, export_vector};
    let opts = RenderOptions {
        fidelity: Fidelity::Approximate,
        ..Default::default()
    };
    for mode in 0i16..8 {
        let make = |flag: i16| {
            let mut data = direct(32, 1, 2, 8, &[0, 0, 0, 0, 0, 255, 255, 255]);
            data[58..66].copy_from_slice(&rect(1, 2, 3, 6));
            data[66..68].copy_from_slice(&word(mode + flag));
            let mut p = Pict::new(2, 8, 4);
            p.op(0x1a, &words(&[0x3333, 0x5555, 0x7777]));
            p.op(0x31, &rect(0, 0, 4, 8));
            p.op(0x1a, &words(&[0, 0, 0]));
            p.op(0x0c, &words(&[1, 0]));
            p.op(1, &region([0, 2, 4, 5], &[]));
            p.op(0x9a, &data).op(0x9a, &data);
            parse(&p.finish(), Container::Raw, &opts.limits).unwrap()
        };
        let base = make(0);
        let flagged = make(64);
        assert_eq!(
            render(&flagged, &RenderOptions::default())
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        let got = render(&flagged, &opts).unwrap();
        assert_eq!(got.image.rgb(), render(&base, &opts).unwrap().image.rgb());
        assert!(
            got.diagnostics
                .iter()
                .any(|d| d.code == "copybits_dither_inactive")
        );
        for format in [VectorFormat::Svg, VectorFormat::Pdf] {
            assert_eq!(
                export_vector(&flagged, &opts, 72., format).is_ok(),
                matches!(mode, 0 | 4)
            );
        }
    }
}

#[test]
fn inactive_dither_also_accepts_monochrome_and_rgb555_without_color_reduction() {
    for (op, mut data, expected, mode_offset) in [
        (
            0x90,
            mono(2, [0, 0, 1, 2], [0, 0, 1, 2], 0, &[0x80, 0]),
            vec![0, 0, 0, 255, 255, 255],
            26,
        ),
        (
            0x9a,
            direct(16, 1, 2, 4, &words(&[0x7c00, 0x03e0])),
            vec![255, 0, 0, 0, 255, 0],
            66,
        ),
    ] {
        data[mode_offset..mode_offset + 2].copy_from_slice(&word(64));
        let mut p = Pict::new(2, 2, 1);
        p.op(op, &data);
        let p = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
        let result = render(
            &p,
            &RenderOptions {
                fidelity: quickdraw::Fidelity::Approximate,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.image.rgb(), expected);
    }
}

#[test]
fn dither_reduction_and_unsupported_combinations_remain_errors() {
    let mut data = direct(32, 1, 2, 8, &[0, 0, 0, 0, 0, 255, 255, 255]);
    data[58..66].copy_from_slice(&rect(0, 0, 1, 1));
    data[66..68].copy_from_slice(&word(64));
    assert_eq!(
        pixels(0x9a, &data, 1, 1).unwrap_err().code,
        ErrorCode::Unsupported
    );
    let mut data = pixmap_header(2, [0, 0, 1, 1], 1, 0, false, 1);
    data.extend([0; 8]);
    data.extend(words(&[0, 0x1200, 0, 0]));
    data.extend(rect(0, 0, 1, 1));
    data.extend(rect(0, 0, 1, 1));
    data.extend(word(64));
    data.extend([0, 0]);
    assert_eq!(
        pixels(0x90, &data, 1, 1).unwrap_err().code,
        ErrorCode::Unsupported
    );
    // Arithmetic+dither combinations are not part of the accepted source-mode profile.
    let data = mono(2, [0, 0, 1, 1], [0, 0, 1, 1], 96, &[0, 0]);
    assert_eq!(
        pixels(0x90, &data, 1, 1).unwrap_err().code,
        ErrorCode::Unsupported
    );
}
