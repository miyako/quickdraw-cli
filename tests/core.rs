mod common;
use common::*;
use quickdraw::{Container, ErrorCode, Fidelity, Limits, RenderOptions, parse, render};

fn draw(bytes: &[u8]) -> quickdraw::Rendered {
    render(
        &parse(bytes, Container::Raw, &Limits::default()).unwrap(),
        &RenderOptions::default(),
    )
    .unwrap()
}
fn rgb(rendered: &quickdraw::Rendered, x: usize, y: usize) -> [u8; 3] {
    let i = (y * rendered.image.width() as usize + x) * 3;
    rendered.image.rgb()[i..i + 3].try_into().unwrap()
}
fn failure(bytes: &[u8]) -> quickdraw::Error {
    parse(bytes, Container::Raw, &Limits::default()).unwrap_err()
}

#[test]
fn v1_and_v2_rectangles_have_identical_ordered_pixels() {
    let mut results = Vec::new();
    for version in [1, 2] {
        let mut p = Pict::new(version, 4, 3);
        p.op(0x31, &rect(0, 0, 3, 4)).op(0x32, &rect(1, 1, 2, 3));
        let rendered = draw(&p.finish());
        assert_eq!(rgb(&rendered, 0, 0), [0; 3]);
        assert_eq!(rgb(&rendered, 1, 1), [255; 3]);
        assert_eq!(rgb(&rendered, 3, 2), [0; 3]);
        results.push(rendered.image.rgb().to_vec());
    }
    assert_eq!(results[0], results[1]);
}

#[test]
fn version2_word_padding_and_comment_payload_are_retained() {
    let mut p = Pict::new(2, 2, 2);
    p.op(0xa1, &[0, 100, 0, 3, 0x41, 0, 0xff])
        .op(0x31, &rect(0, 0, 1, 1));
    let bytes = p.finish();
    let parsed = parse(&bytes, Container::Raw, &Limits::default()).unwrap();
    let info = parsed.inspect();
    assert_eq!(info.operations[0].offset, 40);
    assert_eq!(info.operations[0].data_hex.as_deref(), Some("4100ff"));
    assert_eq!(info.operations[1].offset, 50);
    assert_eq!(
        render(&parsed, &RenderOptions::default()).unwrap_err().code,
        ErrorCode::Unsupported
    );
    let options = RenderOptions {
        fidelity: Fidelity::Approximate,
        ..Default::default()
    };
    let approximate = render(&parsed, &options).unwrap();
    assert!(
        approximate
            .diagnostics
            .iter()
            .any(|d| d.code == "approximation")
    );
    assert_eq!(rgb(&approximate, 0, 0), [0; 3]);
}

#[test]
fn invalid_padding_is_not_misread_as_opcode() {
    let mut p = Pict::new(2, 2, 2);
    p.op(4, &[1]);
    let mut bytes = p.finish();
    bytes[43] = 42;
    assert_eq!(failure(&bytes).offset, Some(43));
}

#[test]
fn header_container_detection_and_absolute_offsets() {
    let mut p = Pict::new(2, 2, 2);
    p.op(0x31, &rect(0, 0, 1, 1));
    let raw = p.finish();
    let mut file = vec![0x5a; 512];
    file.extend(&raw);
    assert_eq!(
        parse(&raw, Container::Auto, &Limits::default())
            .unwrap()
            .inspect()
            .source_offset,
        0
    );
    let info = parse(&file, Container::Auto, &Limits::default())
        .unwrap()
        .inspect();
    assert_eq!(info.container, Container::File);
    assert_eq!(info.operations[0].offset, 552);
    assert!(parse(&file, Container::Raw, &Limits::default()).is_err());
    assert!(parse(&raw, Container::File, &Limits::default()).is_err());
    let mut ambiguous = vec![0; 512];
    ambiguous[..raw.len()].copy_from_slice(&raw);
    ambiguous.extend(raw);
    assert!(
        parse(&ambiguous, Container::Auto, &Limits::default())
            .unwrap_err()
            .message
            .contains("ambiguous")
    );
}

#[test]
fn truncation_at_every_byte_is_an_error_not_a_panic() {
    let mut p = Pict::new(2, 16, 2);
    p.op(
        0x98,
        &mono(
            8,
            [0, 0, 2, 16],
            [0, 0, 2, 16],
            0,
            &[2, 249, 0x55, 2, 249, 0xaa],
        ),
    );
    let bytes = p.finish();
    draw(&bytes);
    for end in 0..bytes.len() {
        assert!(
            parse(&bytes[..end], Container::Raw, &Limits::default()).is_err(),
            "prefix {end}"
        );
    }
}

#[test]
fn deterministic_mutation_sweep_never_panics_or_escapes_limits() {
    let mut p = Pict::new(2, 8, 8);
    p.op(1, &region([0, 0, 8, 8], &[(0, &[0, 8]), (8, &[0, 8])]));
    p.op(0x31, &rect(0, 0, 8, 8));
    let bytes = p.finish();
    let limits = Limits {
        max_pixels: 256,
        max_decoded_bytes: 8192,
        max_work: 4096,
        max_operations: 64,
        ..Default::default()
    };
    for i in 0..bytes.len() {
        for value in [0, 1, 0x7f, 0x80, 0xff] {
            let mut mutated = bytes.clone();
            mutated[i] = value;
            if let Ok(picture) = parse(&mutated, Container::Raw, &limits) {
                let _ = render(
                    &picture,
                    &RenderOptions {
                        limits: limits.clone(),
                        ..Default::default()
                    },
                );
            }
        }
    }
}

#[test]
fn input_pixels_operations_and_decoded_memory_are_bounded() {
    let mut p = Pict::new(2, 10, 10);
    p.op(0, &[]).op(0, &[]);
    let bytes = p.finish();
    for limits in [
        Limits {
            max_input_bytes: bytes.len() - 1,
            ..Default::default()
        },
        Limits {
            max_pixels: 99,
            ..Default::default()
        },
        Limits {
            max_operations: 1,
            ..Default::default()
        },
        Limits {
            max_decoded_bytes: 299,
            ..Default::default()
        },
    ] {
        assert_eq!(
            parse(&bytes, Container::Raw, &limits).unwrap_err().code,
            ErrorCode::ResourceLimit
        );
    }
    assert_eq!(
        parse(
            &bytes,
            Container::Raw,
            &Limits {
                max_work: 0,
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidArguments
    );
}

#[test]
fn cumulative_shape_work_and_scaling_are_bounded() {
    let mut p = Pict::new(2, 10, 10);
    for _ in 0..10 {
        p.op(0x31, &rect(0, 0, 10, 10));
    }

    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let options = RenderOptions {
        limits: Limits {
            max_work: 150,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        render(&pic, &options).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
    let options = RenderOptions {
        width: Some(u32::MAX),
        height: Some(u32::MAX),
        ..Default::default()
    };
    assert_eq!(
        render(&pic, &options).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn empty_offscreen_geometry_preserves_pixels_and_consumes_operation_budget() {
    let mut p = Pict::new(2, 2, 2);
    for _ in 0..8 {
        p.op(0x31, &rect(-32768, 10, 32767, 20));
    }
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let options = RenderOptions {
        limits: Limits {
            max_work: 11,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        render(&pic, &options).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
    let image = render(&pic, &RenderOptions::default()).unwrap();
    assert_eq!(image.image.rgb(), &[255; 12]);
}

#[test]
fn v1_size_and_endpic_are_checked_but_v2_size_is_advisory() {
    let mut bytes = Pict::new(1, 1, 1).finish();
    bytes[1] += 1;
    assert_eq!(failure(&bytes).code, ErrorCode::Malformed);
    let mut v2 = Pict::new(2, 1, 1).finish();
    v2[..2].copy_from_slice(&[0xff, 0xff]);
    draw(&v2);
    v2.extend([0]);
    assert!(failure(&v2).message.contains("trailing"));
}

#[test]
fn version_one_rejects_version_two_only_state_and_unknown_reserved_layouts() {
    for (opcode, data) in [(0x1a, vec![0; 6]), (0xd0, vec![0; 4])] {
        let mut p = Pict::new(1, 1, 1);
        p.op(opcode, &data);
        assert_eq!(failure(&p.finish()).code, ErrorCode::Unsupported);
    }
}

#[test]
fn extended_header_resolution_and_source_rectangle() {
    let mut bytes = Pict::new(2, 3, 2).finish();
    bytes[16..20].copy_from_slice(&[0xff, 0xfe, 0, 0]);
    bytes[20..24].copy_from_slice(&(144u32 << 16).to_be_bytes());
    bytes[24..28].copy_from_slice(&(72u32 << 16).to_be_bytes());
    bytes[28..36].copy_from_slice(&rect(0, 0, 2, 3));
    assert_eq!(
        parse(&bytes, Container::Raw, &Limits::default())
            .unwrap()
            .inspect()
            .resolution_dpi,
        Some([144.0, 72.0])
    );
    bytes[35] = 4;
    assert_eq!(failure(&bytes).code, ErrorCode::Unsupported);
    bytes[20..24].copy_from_slice(&[0; 4]);
    assert_eq!(failure(&bytes).code, ErrorCode::Malformed);
}

#[test]
fn frame_fill_and_background_patterns_have_distinct_state() {
    let mut p = Pict::new(2, 5, 5);
    p.op(0x30, &rect(0, 0, 5, 5));
    p.op(0x0a, &[0xaa; 8]).op(0x34, &rect(1, 1, 4, 4));
    let r = draw(&p.finish());
    assert_eq!(rgb(&r, 0, 2), [0; 3]);
    assert_eq!(rgb(&r, 1, 2), [255; 3]);
    assert_eq!(rgb(&r, 2, 2), [0; 3]);
    assert_eq!(rgb(&r, 4, 2), [0; 3]);
}

#[test]
fn pen_dimensions_and_clipping_do_not_change_with_output_scaling() {
    let mut p = Pict::new(2, 4, 4);
    p.op(7, &words(&[2, 1]))
        .op(1, &region([1, 1, 4, 4], &[]))
        .op(0x30, &rect(0, 0, 4, 4));
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let r = render(
        &pic,
        &RenderOptions {
            width: Some(8),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!((r.image.width(), r.image.height()), (8, 8));
    assert_eq!(rgb(&r, 0, 0), [255; 3]);
    assert_eq!(rgb(&r, 2, 2), [0; 3]);
}

#[test]
fn region_holes_and_negative_coordinates_use_xor_row_transitions() {
    let mut p = Pict::framed(2, [-2, -2, 2, 2]);
    let clip = region(
        [-2, -2, 2, 2],
        &[(-2, &[-2, 2]), (-1, &[-1, 1]), (1, &[-1, 1]), (2, &[-2, 2])],
    );
    p.op(1, &clip).op(0x31, &rect(-2, -2, 2, 2));
    let r = draw(&p.finish());
    assert_eq!(rgb(&r, 0, 0), [0; 3]);
    assert_eq!(rgb(&r, 1, 1), [255; 3]);
    assert_eq!(rgb(&r, 2, 2), [255; 3]);
    assert_eq!(rgb(&r, 3, 3), [0; 3]);
}

#[test]
fn malformed_region_lengths_order_bounds_and_closure_fail() {
    let cases = [
        words(&[8, 0, 0, 4, 4]),
        region([0, 0, 4, 4], &[(0, &[0, 4]), (0, &[0, 4])]),
        region([0, 0, 4, 4], &[(0, &[0, 5]), (4, &[0, 5])]),
        region([0, 0, 4, 4], &[(0, &[0, 4])]),
        region([0, 0, 4, 4], &[(0, &[0])]),
    ];
    for data in cases {
        let mut p = Pict::new(2, 4, 4);
        p.op(1, &data);
        assert_eq!(failure(&p.finish()).code, ErrorCode::Malformed);
    }
}

#[test]
fn every_boolean_mode_composites_against_previous_destination() {
    // Truth tables use logical ink: 1 means black. These expectations are independent
    // of the renderer's RGB implementation, but are not an emulator rendering oracle.
    let expected = [
        [0, 0, 1, 1],
        [0, 1, 1, 1],
        [0, 1, 1, 0],
        [0, 1, 0, 0],
        [1, 1, 0, 0],
        [1, 1, 0, 1],
        [1, 0, 0, 1],
        [0, 0, 0, 1],
    ];
    for mode in 0..8 {
        let mut p = Pict::new(2, 4, 1);
        p.op(
            0x90,
            &mono(2, [0, 0, 1, 4], [0, 0, 1, 4], 0, &[0b01010000, 0]),
        );
        p.op(
            0x90,
            &mono(2, [0, 0, 1, 4], [0, 0, 1, 4], mode, &[0b00110000, 0]),
        );
        let r = draw(&p.finish());
        for (x, &ink) in expected[mode as usize].iter().enumerate() {
            assert_eq!(
                rgb(&r, x, 0),
                [if ink == 1 { 0 } else { 255 }; 3],
                "mode {mode}, pixel {x}"
            );
        }
    }
}

#[test]
fn xor_twice_restores_destination_instead_of_transparent_overlay() {
    let mut p = Pict::new(2, 3, 1);
    p.op(0x31, &rect(0, 0, 1, 2)).op(8, &word(10));
    p.op(0x31, &rect(0, 1, 1, 3)).op(0x31, &rect(0, 1, 1, 3));
    let r = draw(&p.finish());
    assert_eq!(rgb(&r, 0, 0), [0; 3]);
    assert_eq!(rgb(&r, 1, 0), [0; 3]);
    assert_eq!(rgb(&r, 2, 0), [255; 3]);
}

#[test]
fn invert_does_not_use_pen_pattern_or_color() {
    let mut p = Pict::new(2, 2, 1);
    p.op(0x1a, &words(&[-1, 0, 0]))
        .op(0x31, &rect(0, 0, 1, 1))
        .op(9, &[0; 8])
        .op(0x33, &rect(0, 0, 1, 2));
    let r = draw(&p.finish());
    assert_eq!(rgb(&r, 0, 0), [0, 255, 255]);
    assert_eq!(rgb(&r, 1, 0), [0; 3]);
}

#[test]
fn fill_ignores_pen_xor_and_zero_pen_only_suppresses_frames_and_lines() {
    let mut p = Pict::new(2, 2, 2);
    p.op(7, &words(&[0, 0]))
        .op(0x30, &rect(0, 0, 2, 2))
        .op(8, &word(10));
    p.op(0x34, &rect(0, 0, 1, 1)).op(0x34, &rect(0, 0, 1, 1));
    let r = draw(&p.finish());
    assert_eq!(rgb(&r, 0, 0), [0; 3]);
    assert_eq!(rgb(&r, 1, 1), [255; 3]);
}

#[test]
fn thick_xor_line_stamps_union_coverage_once() {
    let mut p = Pict::new(2, 5, 3);
    p.op(7, &words(&[2, 2]))
        .op(8, &word(10))
        .op(0x20, &words(&[0, 0, 0, 3]));
    let r = draw(&p.finish());
    for x in 0..5 {
        assert_eq!(rgb(&r, x, 0), [0; 3]);
    }
    assert_eq!(rgb(&r, 0, 2), [255; 3]);
}

#[test]
fn same_oval_reuses_rectangle_from_another_shape_family_and_requires_opt_in() {
    let mut p = Pict::new(2, 4, 4);
    p.op(0x32, &rect(0, 0, 4, 4)).op(0x59, &[]);
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    assert_eq!(
        render(&pic, &RenderOptions::default()).unwrap_err().code,
        ErrorCode::Unsupported
    );
    let r = render(
        &pic,
        &RenderOptions {
            fidelity: Fidelity::Approximate,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rgb(&r, 0, 0), [255; 3]);
    assert_eq!(rgb(&r, 1, 1), [0; 3]);
}

#[test]
fn polygon_even_odd_fill_and_diagonal_lines_are_explicit_approximations() {
    let mut p = Pict::new(2, 4, 4);
    let mut poly = words(&[22, 0, 0, 4, 4]);
    poly.extend(words(&[0, 0, 0, 4, 4, 0]));
    p.op(0x71, &poly);
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let r = render(
        &pic,
        &RenderOptions {
            fidelity: Fidelity::Approximate,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rgb(&r, 0, 0), [0; 3]);
    assert_eq!(rgb(&r, 3, 3), [255; 3]);
    let mut line = Pict::new(2, 4, 4);
    line.op(0x20, &words(&[0, 0, 3, 3]));
    let pic = parse(&line.finish(), Container::Raw, &Limits::default()).unwrap();
    assert!(render(&pic, &RenderOptions::default()).is_err());
    let r = render(
        &pic,
        &RenderOptions {
            fidelity: Fidelity::Approximate,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rgb(&r, 2, 2), [0; 3]);
    assert_eq!(rgb(&r, 2, 1), [255; 3]);
}

#[test]
fn unsupported_drawing_is_never_silently_discarded() {
    for (op, data) in [
        (0x28, vec![0, 0, 0, 0, 1, b'A']),
        (0x8200, vec![0, 0, 0, 2, 1, 2]),
    ] {
        let mut p = Pict::new(2, 4, 4);
        p.op(op, &data);
        let parsed = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
        for fidelity in [Fidelity::Strict, Fidelity::Approximate] {
            let e = render(
                &parsed,
                &RenderOptions {
                    fidelity,
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert_eq!(e.code, ErrorCode::Unsupported);
            assert_eq!(e.opcode, Some(op));
            assert_eq!(e.offset, Some(40));
        }
    }
}

#[test]
fn reserved_length_classes_are_parsed_with_alignment_and_diagnostics() {
    let mut p = Pict::new(2, 2, 2);
    p.op(0x0100, &[1, 2])
        .op(0x8000, &[])
        .op(0x8101, &[0, 0, 0, 1, 9])
        .op(0x31, &rect(0, 0, 1, 1));
    let bytes = p.finish();
    let r = draw(&bytes);
    assert_eq!(rgb(&r, 0, 0), [0; 3]);
    assert_eq!(
        r.diagnostics
            .iter()
            .filter(|d| d.code == "reserved_opcode")
            .count(),
        3
    );
    let mut p = Pict::new(2, 1, 1);
    p.op(0x8101, &[0xff; 4]);
    assert_eq!(failure(&p.finish()).code, ErrorCode::Malformed);
}

#[test]
fn short_line_deltas_are_signed_and_overflow_is_rejected() {
    let mut p = Pict::new(2, 4, 1);
    p.op(0x22, &[0, 0, 0, 3, 0xfd, 0]);
    let r = draw(&p.finish());
    for x in 0..4 {
        assert_eq!(rgb(&r, x, 0), [0; 3]);
    }
    let mut p = Pict::new(2, 1, 1);
    p.op(0x22, &[0, 0, 0x7f, 0xff, 1, 0]);
    assert!(failure(&p.finish()).message.contains("overflow"));
}

#[test]
fn rendering_and_png_encoding_are_repeatable_and_dpi_is_metadata() {
    let mut p = Pict::new(2, 2, 1);
    p.op(0x31, &rect(0, 0, 1, 1));
    let bytes = p.finish();
    let a = draw(&bytes);
    let b = draw(&bytes);
    let png = quickdraw::encode_png(&a.image, 144.0).unwrap();
    assert_eq!(png, quickdraw::encode_png(&b.image, 144.0).unwrap());
    let decoder = png::Decoder::new(png.as_slice());
    let mut reader = decoder.read_info().unwrap();
    assert_eq!(reader.info().pixel_dims.unwrap().xppu, 5669);
    let mut decoded = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut decoded).unwrap();
    assert_eq!(&decoded[..info.buffer_size()], a.image.rgb());
    assert!(quickdraw::encode_png(&a.image, f64::NAN).is_err());
}
