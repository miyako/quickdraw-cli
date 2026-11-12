mod common;
use common::*;
use quickdraw::{
    Container, ErrorCode, Fidelity, Limits, RenderOptions, VectorFormat, export_vector, parse,
};

fn svg(p: Pict, options: &RenderOptions) -> String {
    let picture = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    String::from_utf8(
        export_vector(&picture, options, 72.0, VectorFormat::Svg)
            .unwrap()
            .bytes,
    )
    .unwrap()
}
#[test]
fn preserves_geometry_origin_clip_and_interior_hole_without_embedded_raster() {
    let mut p = Pict::new(2, 20, 10);
    p.op(1, &region([0, 2, 10, 18], &[]));
    p.op(0x31, &rect(0, 0, 2, 2)).op(0x0c, &words(&[4, 2]));
    p.op(7, &words(&[1, 2])).op(0x30, &rect(2, 4, 8, 16));
    let s = svg(p, &RenderOptions::default());
    assert!(!s.contains("<image"));
    assert!(s.contains("translate(-4 -2)"));
    assert!(s.contains("clipPathUnits=\"userSpaceOnUse\""));
    assert!(s.contains("M4.00000000 2.00000000"));
    assert!(s.contains("M6.00000000 3.00000000"));
    assert!(s.contains("fill-rule=\"evenodd\""));
}
#[test]
fn curves_emit_beziers_and_negative_arc_sweep_keeps_direction() {
    let mut p = Pict::new(2, 20, 10);
    p.op(0x0b, &words(&[4, 6])).op(0x40, &rect(0, 0, 10, 20));
    let mut arc = rect(0, 0, 10, 20);
    arc.extend(words(&[0, -90]));
    p.op(0x61, &arc);
    let s = svg(
        p,
        &RenderOptions {
            fidelity: Fidelity::Approximate,
            ..Default::default()
        },
    );
    assert!(s.contains('C'));
    assert!(s.contains("0.00000000 5.00000000")); // counterclockwise terminates at left midpoint
    assert!(!s.contains("base64"));
}
#[test]
fn destination_modes_patterns_and_missing_fonts_never_disappear_in_vectors() {
    for (opcode, data) in [(8, word(10)), (9, vec![0xaa; 8])] {
        let mut p = Pict::new(2, 2, 2);
        p.op(opcode, &data).op(0x31, &rect(0, 0, 2, 2));
        let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
        let e =
            export_vector(&pic, &RenderOptions::default(), 72.0, VectorFormat::Pdf).unwrap_err();
        assert_eq!(e.code, ErrorCode::Unsupported);
        assert_eq!(e.opcode, Some(0x31));
    }
    let mut p = Pict::new(2, 2, 2);
    p.op(0x28, &[0, 1, 0, 0, 1, b'A']);
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    assert!(
        export_vector(&pic, &RenderOptions::default(), 72.0, VectorFormat::Svg)
            .unwrap_err()
            .message
            .contains("no explicit NFNT")
    );
}
#[test]
fn region_runs_and_path_serialization_obey_work_and_memory_limits() {
    let mut p = Pict::new(2, 20, 10);
    p.op(0x31, &rect(0, 0, 10, 20));
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    for limits in [
        Limits {
            max_work: 2,
            ..Default::default()
        },
        Limits {
            max_decoded_bytes: 1000,
            ..Default::default()
        },
    ] {
        let options = RenderOptions {
            limits,
            ..Default::default()
        };
        assert_eq!(
            export_vector(&pic, &options, 72.0, VectorFormat::Svg)
                .unwrap_err()
                .code,
            ErrorCode::ResourceLimit
        );
    }
}
#[test]
fn output_is_deterministic_with_explicit_physical_size_and_pdf_bounds() {
    let mut p = Pict::new(2, 20, 10);
    p.op(0x31, &rect(0, 0, 10, 20));
    let pic = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let options = RenderOptions {
        width: Some(40),
        ..Default::default()
    };
    for format in [VectorFormat::Svg, VectorFormat::Pdf] {
        let a = export_vector(&pic, &options, 144.0, format).unwrap();
        let b = export_vector(&pic, &options, 144.0, format).unwrap();
        assert_eq!(a.bytes, b.bytes);
        assert_eq!((a.width, a.height), (40, 20));
        assert!(
            a.diagnostics
                .iter()
                .any(|d| d.code == "unverified_fidelity")
        );
        assert_eq!(
            export_vector(&pic, &options, 1e-310, format)
                .unwrap_err()
                .code,
            ErrorCode::InvalidArguments
        );
    }
    assert_eq!(
        export_vector(&pic, &options, 0.001, VectorFormat::Pdf)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArguments
    );
}
