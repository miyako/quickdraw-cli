mod common;
use common::*;
use quickdraw::{Container, ErrorCode, Limits, RenderOptions, parse, render};

fn png_bytes(alpha: u8) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255, 0, 0, alpha, 0, 255, 0, 255])
            .unwrap();
    }
    out
}
#[test]
fn opcolor_transparency_key_is_exact_rgb_not_background_or_per_channel_blend() {
    use quickdraw::{Fidelity, VectorFormat, export_vector};
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 4, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[18, 52, 86, 18, 52, 87, 18, 53, 86, 19, 52, 86])
            .unwrap();
    }
    let mut matrix = identity_matrix();
    matrix[6] = 2 * 65536;
    matrix[7] = 65536;
    let mut packet = quicktime(b"png ", 4, 1, &png, matrix, &[]);
    packet[54..56].copy_from_slice(&36u16.to_be_bytes());
    let mut p = Pict::new(2, 6, 2);
    p.op(0x31, &rect(0, 0, 2, 6));
    p.op(0x1b, &words(&[0x7777, 0x7777, 0x7777]));
    p.op(0x0c, &words(&[1, 0]));
    p.op(0x1f, &words(&[0x12ff, 0x3400, 0x5680]));
    p.op(0x8200, &packet);
    // OpColor is ordered state; the second image makes the formerly keyed pixel visible.
    p.op(0x1f, &words(&[0, 0, 0]));
    let mut second = packet.clone();
    second[34..38].copy_from_slice(&0i32.to_be_bytes());
    p.op(0x8200, &second);
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let opts = RenderOptions {
        fidelity: Fidelity::Approximate,
        ..Default::default()
    };
    let image = render(&pic, &opts).unwrap().image;
    assert_eq!(
        &image.rgb()[18..36],
        &[
            0, 0, 0, 0, 0, 0, 18, 52, 87, 18, 53, 86, 19, 52, 86, 0, 0, 0
        ]
    );
    assert_eq!(&image.rgb()[3..6], &[18, 52, 86]);
    for format in [VectorFormat::Svg, VectorFormat::Pdf] {
        assert!(export_vector(&pic, &opts, 72., format).is_ok());
    }
    let mut missing = Pict::new(2, 6, 2);
    missing.op(0x8200, &packet);
    let pic = parse(&missing.finish(), Container::Raw, &Limits::default()).unwrap();
    assert_eq!(
        render(&pic, &opts).unwrap_err().code,
        ErrorCode::Unsupported
    );
    let mut alpha = quicktime(b"png ", 2, 1, &png_bytes(128), identity_matrix(), &[]);
    alpha[54..56].copy_from_slice(&36u16.to_be_bytes());
    assert_eq!(decode(&alpha).unwrap_err().code, ErrorCode::Unsupported);
}
fn decode(payload: &[u8]) -> quickdraw::Result<quickdraw::Rendered> {
    let mut p = Pict::new(2, 8, 4);
    p.op(0x8200, payload);
    render(
        &parse(&p.finish(), Container::Raw, &Limits::default())?,
        &RenderOptions::default(),
    )
}
#[test]
fn actual_png_decode_preserves_translation_scaling_origin_source_mask_and_clip() {
    let mask = region([0, 0, 1, 2], &[(0, &[1, 2]), (1, &[1, 2])]);
    let mut matrix = identity_matrix();
    matrix[0] = 2 * 65536;
    matrix[6] = 2 * 65536;
    matrix[7] = 65536;
    let payload = quicktime(b"png ", 2, 1, &png_bytes(255), matrix, &mask[10..]);
    let mut p = Pict::new(2, 8, 3);
    p.op(0x31, &rect(0, 0, 3, 8)); // black underneath the masked image
    p.op(0x1a, &words(&[-1, 0, -1])); // image copy must not inherit CopyBits tint
    p.op(0x0c, &words(&[1, 0]));
    p.op(1, &region([0, 0, 3, 5], &[])).op(0x8200, &payload);
    let image = render(
        &parse(&p.finish(), Container::Raw, &Limits::default()).unwrap(),
        &RenderOptions::default(),
    )
    .unwrap()
    .image;
    assert_eq!(
        &image.rgb()[24..48],
        &[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]
    );
}
#[test]
fn actual_jpeg_decode_is_bounded_and_places_the_encoded_pixels() {
    let mut encoded = Vec::new();
    jpeg_encoder::Encoder::new(&mut encoded, 100)
        .encode(&[255, 0, 0, 255, 0, 0], 2, 1, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    let image = decode(&quicktime(b"jpeg", 2, 1, &encoded, identity_matrix(), &[]))
        .unwrap()
        .image;
    for rgb in image.rgb()[..6].chunks_exact(3) {
        assert!(rgb[0] >= 250 && rgb[1] <= 5 && rgb[2] <= 5, "{rgb:?}");
    }
    assert_eq!(&image.rgb()[6..9], &[255; 3]);
}
#[test]
fn unsupported_copy_alpha_codec_projective_matte_and_modes_are_not_silently_changed() {
    let valid = quicktime(b"png ", 2, 1, &png_bytes(255), identity_matrix(), &[]);
    assert_eq!(
        decode(&quicktime(
            b"png ",
            2,
            1,
            &png_bytes(128),
            identity_matrix(),
            &[]
        ))
        .unwrap_err()
        .code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        decode(&quicktime(b"cvid", 2, 1, &[0; 4], identity_matrix(), &[]))
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    for (at, value) in [(14, 65536i32), (38, 0), (42, 1)] {
        // perspective, projective W, separate matte size
        let mut bad = valid.clone();
        bad[at..at + 4].copy_from_slice(&value.to_be_bytes());
        assert_eq!(decode(&bad).unwrap_err().code, ErrorCode::Unsupported);
    }

    let mut bad = valid;
    bad[54..56].copy_from_slice(&2u16.to_be_bytes());
    assert_eq!(decode(&bad).unwrap_err().code, ErrorCode::Unsupported);
}

fn direct_payload(
    codec: &[u8; 4],
    width: u16,
    height: u16,
    depth: u16,
    mode: u16,
    bytes: &[u8],
    matrix: [i32; 9],
) -> Vec<u8> {
    let mut payload = quicktime(codec, width, height, bytes, matrix, &[]);
    payload[54..56].copy_from_slice(&mode.to_be_bytes());
    payload[154..156].copy_from_slice(&depth.to_be_bytes());
    payload
}

fn animation(body: &[u8], header: &[u8]) -> Vec<u8> {
    let mut bytes = ((4 + header.len() + body.len()) as u32)
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(body);
    bytes
}

fn portable(payload: &[u8], backdrop: [i16; 3]) -> quickdraw::Result<quickdraw::Rendered> {
    let mut p = Pict::new(2, 8, 4);
    p.op(0x1a, &words(&backdrop))
        .op(0x31, &rect(0, 0, 4, 8))
        .op(0x8200, payload);
    render(
        &parse(&p.finish(), Container::Raw, &Limits::default())?,
        &RenderOptions {
            fidelity: quickdraw::Fidelity::Approximate,
            ..Default::default()
        },
    )
}

fn extension(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut data = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    data.extend(kind);
    data.extend(body);
    data
}
fn neutral_extensions() -> Vec<u8> {
    [
        extension(b"fiel", &[1, 0]),
        extension(b"pasp", &[0, 0, 0, 2, 0, 0, 0, 2]),
        extension(
            b"clap",
            &[2i32, 2, 3, 3, 0, 1, 0, 2]
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<_>>(),
        ),
        vec![0; 4],
    ]
    .concat()
}

#[test]
fn neutral_description_extensions_and_terminator_preserve_pixels_and_inspection() {
    let base = direct_payload(b"raw ", 1, 1, 24, 0, &[12, 34, 56, 0], identity_matrix());
    for bytes in [
        vec![0; 4],
        neutral_extensions(),
        extension(
            b"clap",
            &[-2i32, -2, -3, -3, 0, -1, 0, -2]
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<_>>(),
        ),
    ] {
        let payload = description_extensions(base.clone(), &bytes);
        let mut p = Pict::new(2, 8, 4);
        let expected_offset = p.bytes.len() + 2 + 158;
        p.op(0x8200, &payload);
        let picture = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
        let inspection = serde_json::to_value(picture.inspect()).unwrap();
        let ext = &inspection["operations"][0]["image_description_extensions"];
        assert_eq!(ext["offset"], expected_offset);
        assert_eq!(
            ext["data_hex"].as_str().unwrap(),
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        let rendered = render(&picture, &RenderOptions::default()).unwrap();
        assert_eq!(&rendered.image.rgb()[..3], &[12, 34, 56]);
        assert!(
            rendered
                .diagnostics
                .iter()
                .any(|d| d.code == "image_description_extensions")
        );
    }
    let data = description_extensions(base, &neutral_extensions());
    let mut p = Pict::new(2, 8, 4);
    p.op(0x8200, &data);
    assert_eq!(
        parse(
            &p.finish(),
            Container::Raw,
            &Limits {
                max_operations: 2,
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn extension_framing_rejects_truncated_duplicate_overlong_and_unknown_semantics() {
    let base = direct_payload(b"raw ", 1, 1, 24, 0, &[1, 2, 3, 0], identity_matrix());
    let valid = neutral_extensions();
    for len in 1..valid.len() {
        if [10, 26, 66].contains(&len) {
            continue;
        }
        assert_eq!(
            decode(&description_extensions(base.clone(), &valid[..len]))
                .unwrap_err()
                .code,
            ErrorCode::Malformed,
            "length {len}"
        );
    }
    for bytes in [
        vec![0; 8],
        vec![0; 5],
        extension(b"fiel", &[1]),
        [extension(b"fiel", &[1, 0]), extension(b"fiel", &[1, 0])].concat(),
        vec![255; 8],
        vec![0, 0, 0, 7, 0, 0, 0, 0],
        extension(b"pasp", &[0; 8]),
        extension(b"clap", &[0; 32]),
        vec![0, 0, 0, 1, 102, 105, 101, 108, 0, 0, 0, 0, 0, 0, 0, 8],
    ] {
        assert_eq!(
            decode(&description_extensions(base.clone(), &bytes))
                .unwrap_err()
                .code,
            ErrorCode::Malformed,
            "{bytes:?}"
        );
    }
    for bytes in [
        extension(b"fiel", &[2, 1]),
        extension(b"fiel", &[1, 9]),
        extension(b"pasp", &[0, 0, 0, 2, 0, 0, 0, 1]),
        extension(
            b"clap",
            &[2i32, 1, 1, 1, 0, 1, 0, 1]
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<_>>(),
        ),
        extension(b"gama", &65536u32.to_be_bytes()),
        extension(b"colr", b"nclc\0\x01\0\x01\0\x01"),
        extension(b"avcC", &[]),
        extension(b"btrt", &[0; 12]),
        extension(b"zzzz", &[]),
        vec![0, 0, 0, 1, 102, 105, 101, 108, 0, 0, 0, 0, 0, 0, 0, 16],
    ] {
        assert_eq!(
            decode(&description_extensions(base.clone(), &bytes))
                .unwrap_err()
                .code,
            ErrorCode::Unsupported,
            "{bytes:?}"
        );
    }
}

#[test]
fn grayscale_depths_have_explicit_white_zero_ramps_not_default_color_palettes() {
    for (depth, bytes, expected) in [
        (34, vec![0x1b, 0], vec![255, 170, 85, 0]),
        (36, vec![0x05, 0xaf], vec![255, 170, 85, 0]),
        (40, vec![0, 85, 170, 255], vec![255, 170, 85, 0]),
    ] {
        let base = direct_payload(b"raw ", 4, 1, depth, 0, &bytes, identity_matrix());
        let image = decode(&base).unwrap().image;
        assert_eq!(
            &image.rgb()[..12],
            expected
                .into_iter()
                .flat_map(|v| [v; 3])
                .collect::<Vec<_>>()
        );
        let mut palette = base.clone();
        palette[156..158].copy_from_slice(&0u16.to_be_bytes());
        assert_eq!(decode(&palette).unwrap_err().code, ErrorCode::Unsupported);
    }
    for depth in [2, 4, 8, 33, 35, 48] {
        let data = direct_payload(b"raw ", 1, 1, depth, 0, &[0, 0], identity_matrix());
        assert_eq!(decode(&data).unwrap_err().code, ErrorCode::Unsupported);
    }
}

#[test]
fn image_blend_requires_recorded_opcolor_and_preserves_channel_weight_endpoints() {
    for mode in [32, 260] {
        let data = direct_payload(
            b"raw ",
            1,
            1,
            32,
            mode,
            &[255, 200, 100, 50],
            identity_matrix(),
        );
        assert_eq!(
            portable(&data, [0; 3]).unwrap_err().code,
            ErrorCode::Unsupported
        );
        let mut p = Pict::new(2, 8, 4);
        p.op(0x1a, &words(&[2570, 5140, 7710]))
            .op(0x31, &rect(0, 0, 4, 8))
            .op(0x1f, &words(&[0, -1, -32768]))
            .op(0x8200, &data);
        let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
        assert_eq!(
            render(&pic, &RenderOptions::default()).unwrap_err().code,
            ErrorCode::Unsupported
        );
        let image = render(
            &pic,
            &RenderOptions {
                fidelity: quickdraw::Fidelity::Approximate,
                ..Default::default()
            },
        )
        .unwrap()
        .image;
        assert_eq!(&image.rgb()[..3], &[10, 100, 40]);
        assert_eq!(&image.rgb()[3..6], &[10, 20, 30]);
    }
    for (mode, depth, bytes) in [
        (32, 32, vec![128, 10, 20, 30]),
        (260, 24, vec![10, 20, 30, 0]),
    ] {
        let data = direct_payload(b"raw ", 1, 1, depth, mode, &bytes, identity_matrix());
        assert_eq!(
            portable(&data, [0; 3]).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
}

#[test]
fn raw_rgb555_rgb24_padding_and_argb_have_declared_channel_order() {
    for (depth, bytes, expected) in [
        (16, vec![0x7c, 0, 0x03, 0xe0], vec![255, 0, 0, 0, 255, 0]),
        (
            24,
            vec![255, 0, 0, 99, 0, 255, 0, 88],
            vec![255, 0, 0, 0, 255, 0],
        ),
        (
            32,
            vec![255, 255, 0, 0, 255, 0, 255, 0],
            vec![255, 0, 0, 0, 255, 0],
        ),
    ] {
        let payload = direct_payload(b"raw ", 1, 2, depth, 0, &bytes, identity_matrix());
        let image = decode(&payload).unwrap().image;
        assert_eq!(&image.rgb()[..3], &expected[..3]);
        assert_eq!(&image.rgb()[24..27], &expected[3..]);
    }
    let payload = direct_payload(b"raw ", 1, 1, 24, 0, &[1, 2, 3], identity_matrix());
    assert_eq!(decode(&payload).unwrap_err().code, ErrorCode::Malformed);
    let payload = direct_payload(b"raw ", 1, 1, 8, 0, &[0, 0], identity_matrix());
    assert_eq!(decode(&payload).unwrap_err().code, ErrorCode::Unsupported);
}

#[test]
fn animation_literal_repeat_full_update_header_and_depths_decode_actual_pixels() {
    for (depth, red, green) in [
        (16, vec![0x7c, 0], vec![0x03, 0xe0]),
        (24, vec![255, 0, 0], vec![0, 255, 0]),
        (32, vec![255, 255, 0, 0], vec![255, 0, 255, 0]),
    ] {
        let mut body = vec![1, 2];
        body.extend(&red);
        body.extend(&green);
        body.extend([0, 1, 255, 1, 254]);
        body.extend(&red);
        body.extend([255, 0]);
        for header in [words(&[0]), words(&[8, 0, 0, 2, 0])] {
            let encoded = animation(&body, &header);
            let image = decode(&direct_payload(
                b"rle ",
                2,
                2,
                depth,
                0,
                &encoded,
                identity_matrix(),
            ))
            .unwrap()
            .image;
            assert_eq!(&image.rgb()[..6], &[255, 0, 0, 0, 255, 0]);
            assert_eq!(&image.rgb()[24..30], &[255, 0, 0, 255, 0, 0]);
        }
    }
}

#[test]
fn animation_truncations_delta_skips_cross_row_runs_and_budgets_are_explicit() {
    let encoded = animation(&[1, 254, 255, 0, 0, 255, 0], &words(&[0]));
    for end in 0..encoded.len() {
        let payload = direct_payload(b"rle ", 2, 1, 24, 0, &encoded[..end], identity_matrix());
        assert!(decode(&payload).is_err(), "prefix {end}");
    }
    for body in [vec![0], vec![2, 255], vec![1, 255], vec![1, 0, 2, 255]] {
        let encoded = animation(&body, &words(&[0]));
        assert_eq!(
            decode(&direct_payload(
                b"rle ",
                2,
                1,
                24,
                0,
                &encoded,
                identity_matrix()
            ))
            .unwrap_err()
            .code,
            ErrorCode::Unsupported
        );
    }
    let too_wide = animation(&[1, 253, 255, 0, 0, 255, 0], &words(&[0]));
    assert_eq!(
        decode(&direct_payload(
            b"rle ",
            2,
            1,
            24,
            0,
            &too_wide,
            identity_matrix()
        ))
        .unwrap_err()
        .code,
        ErrorCode::Malformed
    );
    let partial = animation(&[1, 254, 255, 0, 0, 255, 0], &words(&[8, 1, 0, 1, 0]));
    assert_eq!(
        decode(&direct_payload(
            b"rle ",
            2,
            2,
            24,
            0,
            &partial,
            identity_matrix()
        ))
        .unwrap_err()
        .code,
        ErrorCode::Unsupported
    );
    let mut p = Pict::new(2, 8, 4);
    p.op(
        0x8200,
        &direct_payload(b"rle ", 2, 1, 24, 0, &encoded, identity_matrix()),
    );
    let picture = p.finish();
    for limits in [
        Limits {
            max_decoded_bytes: 4,
            ..Default::default()
        },
        Limits {
            max_work: 2,
            ..Default::default()
        },
        Limits {
            max_operations: 1,
            ..Default::default()
        },
    ] {
        assert_eq!(
            parse(&picture, Container::Raw, &limits).unwrap_err().code,
            ErrorCode::ResourceLimit
        );
    }
}

#[test]
fn straight_and_premultiplied_alpha_compose_against_existing_destination() {
    for (mode, source) in [
        (256, vec![128, 200, 100, 50]),
        (257, vec![128, 227, 177, 152]),
        (258, vec![128, 100, 50, 25]),
    ] {
        let payload = direct_payload(b"raw ", 1, 1, 32, mode, &source, identity_matrix());
        let result = portable(&payload, [0x1414, 0x2828, 0x3c3c]).unwrap();
        let expected = [110, 70, 55];
        assert_eq!(&result.image.rgb()[..3], &expected);
        assert_eq!(&result.image.rgb()[3..6], &[20, 40, 60]);
        assert_eq!(decode(&payload).unwrap_err().code, ErrorCode::Unsupported);
    }
    let mut png = quicktime(b"png ", 2, 1, &png_bytes(128), identity_matrix(), &[]);
    png[54..56].copy_from_slice(&256u16.to_be_bytes());
    assert_eq!(
        &portable(&png, [0, 0, -1]).unwrap().image.rgb()[..6],
        &[128, 0, 127, 0, 255, 0]
    );
    for (mode, source) in [(258, vec![128, 200, 0, 0]), (257, vec![128, 0, 128, 128])] {
        assert_eq!(
            portable(
                &direct_payload(b"raw ", 1, 1, 32, mode, &source, identity_matrix()),
                [0; 3]
            )
            .unwrap_err()
            .code,
            ErrorCode::Malformed
        );
    }
}

#[test]
fn affine_reflection_rotation_shear_fractional_edges_and_singular_errors() {
    let rgb = [255, 0, 0, 0, 255, 0];
    for (matrix, positions) in [
        (
            [-65536, 0, 0, 0, 65536, 0, 2 * 65536, 0, 1 << 30],
            vec![(0, 0, [0, 255, 0]), (1, 0, [255, 0, 0])],
        ),
        (
            [0, 65536, 0, -65536, 0, 0, 65536, 0, 1 << 30],
            vec![(0, 0, [255, 0, 0]), (0, 1, [0, 255, 0])],
        ),
        (
            [65536, 0, 0, 65536, 65536, 0, 0, 0, 1 << 30],
            vec![(0, 0, [255, 0, 0]), (1, 0, [0, 255, 0]), (2, 0, [0, 0, 0])],
        ),
        (
            [65536, 0, 0, 0, 65536, 0, 16384, 0, 1 << 30],
            vec![(0, 0, [255, 0, 0]), (1, 0, [0, 255, 0]), (2, 0, [0, 0, 0])],
        ),
    ] {
        let payload = direct_payload(b"raw ", 2, 1, 24, 0, &rgb, matrix);
        let result = portable(&payload, [0; 3]).unwrap();
        for (x, y, color) in positions {
            assert_eq!(
                &result.image.rgb()[(y * 8 + x) * 3..(y * 8 + x) * 3 + 3],
                &color
            );
        }
        assert!(result.diagnostics.iter().any(|d| d.code == "approximation"));
    }
    let mut singular = identity_matrix();
    singular[0] = 0;
    assert_eq!(
        portable(
            &direct_payload(b"raw ", 2, 1, 24, 0, &rgb, singular),
            [0; 3]
        )
        .unwrap_err()
        .code,
        ErrorCode::Malformed
    );
}
#[test]
fn truncated_payloads_dimension_mismatches_and_decode_limits_fail() {
    let valid = quicktime(b"png ", 2, 1, &png_bytes(255), identity_matrix(), &[]);
    for end in 0..valid.len() {
        assert!(decode(&valid[..end]).is_err(), "{end}");
    }

    let mismatch = quicktime(b"png ", 3, 1, &png_bytes(255), identity_matrix(), &[]);
    assert_eq!(decode(&mismatch).unwrap_err().code, ErrorCode::Malformed);
    let mut p = Pict::new(2, 1, 1);
    p.op(0x8200, &valid);
    assert_eq!(
        parse(
            &p.finish(),
            Container::Raw,
            &Limits {
                max_pixels: 1,
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn alpha_channel_requirements_rle_reserved_bits_and_affine_mutations_remain_bounded() {
    let no_alpha = direct_payload(
        b"raw ",
        2,
        1,
        24,
        256,
        &[255, 0, 0, 0, 255, 0],
        identity_matrix(),
    );
    assert_eq!(
        portable(&no_alpha, [0; 3]).unwrap_err().code,
        ErrorCode::Unsupported
    );
    let rle = animation(&[1, 254, 0xfc, 0, 255, 0], &words(&[0]));
    assert_eq!(
        &decode(&direct_payload(
            b"rle ",
            2,
            1,
            16,
            0,
            &rle,
            identity_matrix()
        ))
        .unwrap()
        .image
        .rgb()[..6],
        &[255, 0, 0, 255, 0, 0]
    );
    let raw = direct_payload(b"raw ", 1, 1, 16, 0, &[0xfc, 0], identity_matrix());
    assert_eq!(decode(&raw).unwrap_err().code, ErrorCode::Unsupported);
    let payload = direct_payload(b"rle ", 2, 1, 16, 0, &rle, identity_matrix());
    for at in 4..payload.len() {
        for bit in [1, 128] {
            let mut mutated = payload.clone();
            mutated[at] ^= bit;
            let _ = portable(&mutated, [0; 3]); // Any accepted variant must render without a panic.
        }
    }
}

#[test]
fn jpeg_truncation_and_extra_images_fail_instead_of_decoding_a_prefix() {
    let mut image = Vec::new();
    jpeg_encoder::Encoder::new(&mut image, 100)
        .encode(&[255, 255, 255], 1, 1, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    for end in [0, 2, image.len() / 2, image.len() - 1] {
        assert_eq!(
            decode(&quicktime(
                b"jpeg",
                1,
                1,
                &image[..end],
                identity_matrix(),
                &[]
            ))
            .unwrap_err()
            .code,
            ErrorCode::Malformed
        );
    }

    image.extend(image.clone());
    assert_eq!(
        decode(&quicktime(b"jpeg", 1, 1, &image, identity_matrix(), &[]))
            .unwrap_err()
            .code,
        ErrorCode::Malformed
    );
}

#[test]
fn codec_payload_corruption_and_non_rgb_jpeg_fail_explicitly() {
    let mut png = png_bytes(255);
    let end = png.len() - 1;
    png[end] ^= 1;
    assert_eq!(
        decode(&quicktime(b"png ", 2, 1, &png, identity_matrix(), &[]))
            .unwrap_err()
            .code,
        ErrorCode::Malformed
    );
    let png = png_bytes(255);
    assert_eq!(
        decode(&quicktime(
            b"png ",
            2,
            1,
            &png[..png.len() - 1],
            identity_matrix(),
            &[]
        ))
        .unwrap_err()
        .code,
        ErrorCode::Malformed
    );
    let mut cmyk = Vec::new();
    jpeg_encoder::Encoder::new(&mut cmyk, 100)
        .encode(&[0, 255, 255, 0], 1, 1, jpeg_encoder::ColorType::Cmyk)
        .unwrap();
    assert_eq!(
        decode(&quicktime(b"jpeg", 1, 1, &cmyk, identity_matrix(), &[]))
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}
