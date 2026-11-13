mod common;
use common::*;
use quickdraw::{Container, ErrorCode, Limits, RenderOptions, parse, render};

fn chunk(marker: u8, body: &[u8]) -> Vec<u8> {
    let mut result = ((body.len() + 4) as u32).to_be_bytes().to_vec();
    result[0] = marker;
    result.extend_from_slice(body);
    result
}

fn table(count: usize) -> Vec<u8> {
    let mut data = vec![0, 0, 0, 0, 0x80, 0];
    data.extend_from_slice(&((count - 1) as u16).to_be_bytes());
    for i in 0..count {
        data.extend_from_slice(&words(&[0, (i as u16 * 257) as i16, 0, 0]));
    }
    data
}

fn payload(
    codec: &[u8; 4],
    width: u16,
    height: u16,
    depth: u16,
    packet: &[u8],
    palette: &[u8],
) -> Vec<u8> {
    let mut data = quicktime(codec, width, height, packet, identity_matrix(), &[]);
    data[72..76].copy_from_slice(&((86 + palette.len()) as u32).to_be_bytes());
    data[154..156].copy_from_slice(&depth.to_be_bytes());
    if !palette.is_empty() {
        data[156..158].copy_from_slice(&0u16.to_be_bytes());
        data.splice(158..158, palette.iter().copied());
        let size = data.len() - 4;
        data[..4].copy_from_slice(&(size as u32).to_be_bytes());
    }
    data
}

fn decode(data: &[u8], width: i16, height: i16, limits: Limits) -> quickdraw::Result<Vec<u8>> {
    let mut p = Pict::new(2, width, height);
    p.op(0x8200, data);
    let pic = parse(&p.finish(), Container::Raw, &limits)?;
    Ok(render(
        &pic,
        &RenderOptions {
            limits,
            ..Default::default()
        },
    )?
    .image
    .rgb()
    .to_vec())
}

fn unhex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .chunks_exact(2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn independently_encoded_stills_match_ffmpeg_decoded_pixels() {
    for json in [
        include_str!("fixtures/quicktime-blocks.json"),
        include_str!("fixtures/quicktime-packed.json"),
    ] {
        let fixtures: serde_json::Value = serde_json::from_str(json).unwrap();
        for case in fixtures["cases"].as_array().unwrap() {
            let codec: &[u8; 4] = case["codec"]
                .as_str()
                .unwrap()
                .as_bytes()
                .try_into()
                .unwrap();
            let width = case["width"].as_u64().unwrap() as u16;
            let height = case["height"].as_u64().unwrap() as u16;
            let data = payload(
                codec,
                width,
                height,
                case["depth"].as_u64().unwrap() as u16,
                &unhex(case["packet"].as_str().unwrap()),
                &unhex(case["palette"].as_str().unwrap()),
            );
            let data =
                description_extensions(data, &unhex(case["extensions"].as_str().unwrap_or("")));
            let rgb = decode(&data, width as i16, height as i16, Limits::default()).unwrap();
            assert_eq!(
                rgb,
                unhex(case["rgb"].as_str().unwrap()),
                "{}",
                case["name"]
            );
        }
    }
}

fn animation(body: &[u8]) -> Vec<u8> {
    let mut data = ((body.len() + 6) as u32).to_be_bytes().to_vec();
    data.extend_from_slice(&[0, 0]);
    data.extend_from_slice(body);
    data
}

#[test]
fn packed_raw_rows_are_high_bit_first_and_word_aligned() {
    for (depth, width, row, indices) in [
        (1, 3, vec![0xa0, 0xff], vec![1, 0, 1]),
        (2, 3, vec![0x6f, 0xff], vec![1, 2, 3]),
        (4, 3, vec![0x12, 0x3f], vec![1, 2, 3]),
        (8, 3, vec![1, 2, 3, 255], vec![1, 2, 3]),
    ] {
        let rows = [row.as_slice(), row.as_slice()].concat();
        let data = payload(
            b"raw ",
            width,
            2,
            depth,
            &rows,
            &table(if depth == 1 { 2 } else { 4 }),
        );
        let rgb = decode(&data, width as i16, 2, Limits::default()).unwrap();
        let expected: Vec<u8> = indices
            .iter()
            .chain(&indices)
            .flat_map(|i| [*i, 0, 0])
            .collect();
        assert_eq!(rgb, expected, "depth {depth}");
    }
}

#[test]
fn indexed_images_require_explicit_palette_and_defined_visible_entries() {
    for codec in [b"raw ", b"rle "] {
        let packet = if codec == b"raw " {
            vec![0x40, 0]
        } else {
            animation(&[1, 1, 0x40, 0, 0, 0, 255, 0])
        };
        let data = payload(codec, 1, 1, 2, &packet, &[]);
        assert_eq!(
            decode(&data, 1, 1, Limits::default()).unwrap_err().code,
            ErrorCode::Unsupported
        );
        let data = payload(codec, 1, 1, 2, &packet, &table(1));
        assert_eq!(
            decode(&data, 1, 1, Limits::default()).unwrap_err().code,
            ErrorCode::Malformed
        );
        let data = payload(codec, 1, 1, 2, &packet, &table(5));
        assert_eq!(
            decode(&data, 1, 1, Limits::default()).unwrap_err().code,
            ErrorCode::Malformed
        );
        let data = payload(codec, 1, 1, 2, &packet, &table(2));
        assert_eq!(decode(&data, 1, 1, Limits::default()).unwrap(), [1, 0, 0]);
    }
}

#[test]
fn packed_animation_groups_crop_only_final_storage_padding() {
    for (depth, group, unit) in [
        (2, 16, vec![0x6c, 0x6c, 0x6c, 0x6c]),
        (4, 8, vec![0x12, 0x30, 0x12, 0x30]),
        (8, 4, vec![1, 2, 3, 0]),
    ] {
        let mut body = vec![1, 1];
        body.extend(&unit);
        body.extend([0, 1, 254]);
        body.extend(&unit);
        body.extend([255, 0]);
        let data = payload(
            b"rle ",
            group * 3 - 1,
            1,
            depth,
            &animation(&body),
            &table(4),
        );
        let rgb = decode(&data, (group * 3 - 1) as i16, 1, Limits::default()).unwrap();
        let expected: Vec<u8> = (0..group * 3 - 1)
            .flat_map(|i| [([1, 2, 3, 0][i as usize % 4]), 0, 0])
            .collect();
        assert_eq!(rgb, expected);
        let data = payload(b"rle ", group * 2, 1, depth, &animation(&body), &table(4));
        assert_eq!(
            decode(&data, (group * 2) as i16, 1, Limits::default())
                .unwrap_err()
                .code,
            ErrorCode::Malformed
        );
    }
}

#[test]
fn monochrome_animation_uses_explicit_row_and_command_pairs() {
    let packet = animation(&[
        0x80, 1, 0xaa, 0xaa, 0, 255, 0, 1, 0x55, 0x55, 0x80, 254, 0xff, 0xff, 0, 0,
    ]);
    let data = payload(b"rle ", 32, 2, 1, &packet, &table(2));
    let rgb = decode(&data, 32, 2, Limits::default()).unwrap();
    let expected: Vec<u8> = (0..64)
        .flat_map(|i| {
            [
                if i >= 32 || (i < 16 && i % 2 == 0) || ((16..32).contains(&i) && i % 2 == 1) {
                    1
                } else {
                    0
                },
                0,
                0,
            ]
        })
        .collect();
    assert_eq!(rgb, expected);
    let limits = Limits {
        max_operations: 2,
        ..Default::default()
    };
    assert_eq!(
        decode(&data, 32, 2, limits).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn packed_animation_never_supplies_previous_frame_pixels() {
    for body in [
        vec![0x81, 1, 0, 0, 0, 0],
        vec![0x80, 255, 0, 0],
        vec![0x80, 1, 0, 0, 0x80, 255, 0, 0],
        vec![0x80, 1, 0, 0, 1, 1, 0, 0, 0, 0],
    ] {
        let data = payload(b"rle ", 32, 1, 1, &animation(&body), &table(2));
        assert_eq!(
            decode(&data, 32, 1, Limits::default()).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
    for depth in [2, 4, 8] {
        for body in [&[2, 255, 0][..], &[1, 255, 0], &[1, 0, 2, 255, 0]] {
            let data = payload(b"rle ", 16, 1, depth, &animation(body), &table(4));
            assert_eq!(
                decode(&data, 16, 1, Limits::default()).unwrap_err().code,
                ErrorCode::Unsupported
            );
        }
    }
}

#[test]
fn packed_packets_are_bounded_and_reject_truncation_flags_and_trailing_bytes() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/quicktime-packed.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let codec: &[u8; 4] = case["codec"]
            .as_str()
            .unwrap()
            .as_bytes()
            .try_into()
            .unwrap();
        let width = case["width"].as_u64().unwrap() as u16;
        let height = case["height"].as_u64().unwrap() as u16;
        let depth = case["depth"].as_u64().unwrap() as u16;
        let packet = unhex(case["packet"].as_str().unwrap());
        let palette = unhex(case["palette"].as_str().unwrap());
        for len in 0..packet.len() {
            if codec == b"rle " && depth != 1 && len == packet.len() - 1 {
                continue;
            } // Optional final byte.
            let mut truncated = packet[..len].to_vec();
            if codec == b"rle " && len >= 4 {
                truncated[..4].copy_from_slice(&(len as u32).to_be_bytes());
            }
            let data = payload(codec, width, height, depth, &truncated, &palette);
            assert!(
                decode(&data, width as i16, height as i16, Limits::default()).is_err(),
                "{} length {len}",
                case["name"]
            );
        }
        let mut extra = packet.clone();
        extra.push(42);
        if codec == b"rle " {
            extra[..4].copy_from_slice(&((packet.len() + 1) as u32).to_be_bytes());
        }
        let data = payload(codec, width, height, depth, &extra, &palette);
        assert!(decode(&data, width as i16, height as i16, Limits::default()).is_err());
        let data = payload(codec, width, height, depth, &packet, &palette);
        let mut alpha = data.clone();
        alpha[54..56].copy_from_slice(&256u16.to_be_bytes());
        assert_eq!(
            decode(&alpha, width as i16, height as i16, Limits::default())
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            decode(
                &data,
                width as i16,
                height as i16,
                Limits {
                    max_pixels: 1,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::ResourceLimit
        );
        if codec == b"rle " {
            let mut update = packet.clone();
            update[4..6].copy_from_slice(&8u16.to_be_bytes());
            update.splice(6..6, words(&[0, 0, height as i16, 0]));
            let size = update.len();
            update[..4].copy_from_slice(&(size as u32).to_be_bytes());
            let data = payload(codec, width, height, depth, &update, &palette);
            assert_eq!(
                decode(&data, width as i16, height as i16, Limits::default()).unwrap(),
                unhex(case["rgb"].as_str().unwrap())
            );
            let mut flags = packet.clone();
            flags[4] = 0x80;
            let data = payload(codec, width, height, depth, &flags, &palette);
            assert_eq!(
                decode(&data, width as i16, height as i16, Limits::default())
                    .unwrap_err()
                    .code,
                ErrorCode::Unsupported
            );
        }
    }
}

#[test]
fn rpza_solid_quad_special_literal_and_partial_edge_blocks() {
    let packet = chunk(0xe1, &[0xa1, 0x7c, 0]);
    let data = payload(b"rpza", 5, 3, 16, &packet, &[]);
    assert_eq!(
        decode(&data, 5, 3, Limits::default()).unwrap(),
        [255, 0, 0].repeat(15)
    );
    for prefix in [vec![0xc0, 0x7c, 0, 0, 0x1f], vec![0x7c, 0, 0x80, 0x1f]] {
        let body = [prefix, vec![0x1b; 4]].concat();
        let data = payload(b"rpza", 4, 4, 16, &chunk(0xe1, &body), &[]);
        assert_eq!(
            decode(&data, 4, 4, Limits::default()).unwrap(),
            [0, 0, 255, 82, 0, 165, 165, 0, 82, 255, 0, 0].repeat(4)
        );
    }
    let body: Vec<u8> = (0..16u16).flat_map(|i| (i << 10).to_be_bytes()).collect();
    let data = payload(b"rpza", 3, 3, 24, &chunk(0xe1, &body), &[]);
    let expected: Vec<u8> = [0u8, 1, 2, 4, 5, 6, 8, 9, 10]
        .into_iter()
        .flat_map(|v| [(v << 3) | (v >> 2), 0, 0])
        .collect();
    assert_eq!(decode(&data, 3, 3, Limits::default()).unwrap(), expected);
}

#[test]
fn smc_literal_current_frame_repeats_and_partial_blocks_preserve_colors() {
    let mut body = vec![0xe0];
    body.extend(0..16u8);
    body.extend([0x60, 19, 0x40, 0x21]);
    let data = payload(b"smc ", 9, 8, 8, &chunk(0, &body), &table(20));
    let rgb = decode(&data, 9, 8, Limits::default()).unwrap();
    let blocks: [[u8; 16]; 6] = [
        std::array::from_fn(|i| i as u8),
        [19; 16],
        std::array::from_fn(|i| i as u8),
        [19; 16],
        [19; 16],
        [19; 16],
    ];
    let expected: Vec<u8> = (0..8)
        .flat_map(|y| {
            (0..9).flat_map(move |x| [blocks[y / 4 * 3 + x / 4][y % 4 * 4 + x % 4], 0, 0])
        })
        .collect();
    assert_eq!(rgb, expected);
}

#[test]
fn smc_pairs_quads_octets_cache_references_and_wrap() {
    for (opcode, colors, flags, expected) in [
        (
            0x80,
            vec![3, 7],
            vec![0xaa, 0x55],
            vec![7, 3, 7, 3, 7, 3, 7, 3, 3, 7, 3, 7, 3, 7, 3, 7],
        ),
        (
            0xa0,
            vec![2, 4, 6, 8],
            vec![0x1b; 4],
            [2, 4, 6, 8].repeat(4),
        ),
        (
            0xc0,
            (0..8).collect(),
            vec![0x01, 0x23, 0x45, 0x67, 0x89, 0xab],
            vec![0, 0, 2, 2, 2, 1, 2, 6, 4, 2, 3, 2, 1, 5, 7, 3],
        ),
    ] {
        let body = [
            vec![opcode],
            colors,
            flags.clone(),
            vec![opcode + 0x10, 0],
            flags,
        ]
        .concat();
        let data = payload(b"smc ", 8, 4, 8, &chunk(0x80, &body), &table(9));
        let rgb = decode(&data, 8, 4, Limits::default()).unwrap();
        let expected: Vec<u8> = expected
            .chunks_exact(4)
            .flat_map(|row| row.iter().chain(row).flat_map(|&i| [i, 0, 0]))
            .collect();
        assert_eq!(rgb, expected, "opcode {opcode:x}");
    }
    let mut body = Vec::new();
    for i in 0..257 {
        body.extend([0x80, (i % 9) as u8, 0, 0, 0]);
    }
    body.extend([0x90, 0, 0, 0]);
    let data = payload(b"smc ", 1032, 4, 8, &chunk(0xe1, &body), &table(9));
    let rgb = decode(&data, 1032, 4, Limits::default()).unwrap();
    assert_eq!(&rgb[1028 * 3..1032 * 3], &[4, 0, 0].repeat(4));
}

#[test]
fn temporal_missing_cache_and_unknown_variants_fail_explicitly() {
    for (codec, depth, marker, body, palette) in [
        (b"rpza", 16, 0xe1, vec![0x80], vec![]),
        (b"rpza", 16, 0xe1, vec![], vec![]),
        (b"rpza", 16, 0xe1, vec![0xe0], vec![]),
        (b"smc ", 8, 0, vec![0x00], table(1)),
        (b"smc ", 8, 0, vec![0x10, 0], table(1)),
        (b"smc ", 8, 0, vec![0x20], table(1)),
        (b"smc ", 8, 0, vec![0x90, 0, 0, 0], table(1)),
        (b"smc ", 8, 0, vec![0xb0, 0, 0, 0, 0, 0], table(1)),
        (b"smc ", 8, 0, vec![0xd0, 0, 0, 0, 0, 0, 0, 0], table(1)),
        (b"smc ", 8, 0, vec![0xf0], table(1)),
    ] {
        let data = payload(codec, 4, 4, depth, &chunk(marker, &body), &palette);
        assert_eq!(
            decode(&data, 4, 4, Limits::default()).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
}

#[test]
fn palette_selection_lengths_and_unavailable_colors_are_explicit() {
    let packet = chunk(0, &[0x60, 0]);
    let mut data = payload(b"smc ", 4, 4, 8, &packet, &[]);
    assert_eq!(
        decode(&data, 4, 4, Limits::default()).unwrap_err().code,
        ErrorCode::Unsupported
    );
    data = payload(b"smc ", 4, 4, 8, &packet, &table(2));
    for (at, value) in [(156, 7u16), (162, 0), (166, 1)] {
        let mut bad = data.clone();
        bad[at..at + 2].copy_from_slice(&value.to_be_bytes());
        assert_eq!(
            decode(&bad, 4, 4, Limits::default()).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
    let data = payload(b"smc ", 4, 4, 8, &chunk(0, &[0x60, 2]), &table(2));
    assert_eq!(
        decode(&data, 4, 4, Limits::default()).unwrap_err().code,
        ErrorCode::Malformed
    );
}

#[test]
fn malformed_runs_truncation_and_resource_limits_never_fill_missing_pixels() {
    for (codec, depth, marker, body, palette) in [
        (b"rpza", 16, 0xe1, vec![0xa0, 0x7c, 0], vec![]),
        (b"smc ", 8, 0, vec![0x60, 0], table(1)),
    ] {
        let packet = chunk(marker, &body);
        for n in 0..packet.len() {
            let data = payload(codec, 4, 4, depth, &packet[..n], &palette);
            assert!(decode(&data, 4, 4, Limits::default()).is_err());
        }
        for n in 0..body.len() {
            let data = payload(codec, 4, 4, depth, &chunk(marker, &body[..n]), &palette);
            assert!(decode(&data, 4, 4, Limits::default()).is_err());
        }
        let mut overrun = body.clone();
        overrun[0] += 1;
        let data = payload(codec, 4, 4, depth, &chunk(marker, &overrun), &palette);
        assert_eq!(
            decode(&data, 4, 4, Limits::default()).unwrap_err().code,
            ErrorCode::Malformed
        );
        let mut trailing = body.clone();
        trailing.push(0);
        let data = payload(codec, 4, 4, depth, &chunk(marker, &trailing), &palette);
        assert_eq!(
            decode(&data, 4, 4, Limits::default()).unwrap_err().code,
            ErrorCode::Malformed
        );
        let data = payload(codec, 4, 4, depth, &packet, &palette);
        for limits in [
            Limits {
                max_pixels: 15,
                ..Default::default()
            },
            Limits {
                max_decoded_bytes: 32,
                ..Default::default()
            },
            Limits {
                max_work: 1,
                ..Default::default()
            },
        ] {
            assert_eq!(
                decode(&data, 4, 4, limits).unwrap_err().code,
                ErrorCode::ResourceLimit
            );
        }
    }
}

#[test]
fn smc_extended_counts_and_independent_cache_initialization() {
    for body in [
        vec![0x70, 17, 2],
        vec![0x60, 2, 0x30, 16],
        vec![0x61, 2, 0x50, 7],
    ] {
        let data = payload(b"smc ", 24, 12, 8, &chunk(0, &body), &table(3));
        assert_eq!(
            decode(&data, 24, 12, Limits::default()).unwrap(),
            [2, 0, 0].repeat(288)
        );
    }
    let first = payload(b"smc ", 4, 4, 8, &chunk(0, &[0x80, 0, 1, 0, 0]), &table(2));
    let second = payload(b"smc ", 4, 4, 8, &chunk(0, &[0x90, 0, 0, 0]), &table(2));
    let mut p = Pict::new(2, 4, 4);
    p.op(0x8200, &first).op(0x8200, &second);
    assert_eq!(
        parse(&p.finish(), Container::Raw, &Limits::default())
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    let data = payload(b"smc ", 8, 4, 8, &chunk(0, &[0x60, 0, 0x60, 1]), &table(2));
    assert_eq!(
        decode(
            &data,
            8,
            4,
            Limits {
                max_operations: 1,
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn palette_bounds_alpha_modes_and_payload_mutations_are_rejected_or_bounded() {
    for codec in [b"smc ", b"rpza"] {
        let (depth, packet, palette) = if codec == b"smc " {
            (
                8,
                chunk(
                    0,
                    &[
                        0xc0, 0, 1, 2, 3, 4, 5, 6, 7, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                    ],
                ),
                table(8),
            )
        } else {
            (
                16,
                chunk(0xe1, &[0xc0, 0x7c, 0, 0, 0x1f, 0x1b, 0x1b, 0x1b, 0x1b]),
                vec![],
            )
        };
        let data = payload(codec, 4, 4, depth, &packet, &palette);
        for mode in [256u16, 257, 258] {
            let mut bad = data.clone();
            bad[54..56].copy_from_slice(&mode.to_be_bytes());
            assert_eq!(
                decode(&bad, 4, 4, Limits::default()).unwrap_err().code,
                ErrorCode::Unsupported
            );
        }
        for at in 72..data.len() {
            let mut bad = data.clone();
            bad[at] ^= 0x80;
            let _ = decode(
                &bad,
                4,
                4,
                Limits {
                    max_decoded_bytes: 1_000_000,
                    ..Default::default()
                },
            );
        }
        for n in 0..packet.len() - 4 {
            let bad = payload(
                codec,
                4,
                4,
                depth,
                &chunk(packet[0], &packet[4..4 + n]),
                &palette,
            );
            assert!(decode(&bad, 4, 4, Limits::default()).is_err());
        }
    }
    for count in [0u16, 256, 65535] {
        let mut palette = table(2);
        palette[6..8].copy_from_slice(&count.to_be_bytes());
        let data = payload(b"smc ", 4, 4, 8, &chunk(0, &[0x60, 0]), &palette);
        assert!(decode(&data, 4, 4, Limits::default()).is_err());
    }
    for n in 0..table(2).len() {
        let data = payload(b"smc ", 4, 4, 8, &chunk(0, &[0x60, 0]), &table(2)[..n]);
        assert!(decode(&data, 4, 4, Limits::default()).is_err());
    }
}
