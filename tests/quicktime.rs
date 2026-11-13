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
fn unsupported_alpha_codec_transform_matte_and_modes_are_not_silently_changed() {
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
    for (at, value) in [(10, 65536i32), (38, 0), (42, 1)] {
        // shear, projective W, matte size
        let mut bad = valid.clone();
        bad[at..at + 4].copy_from_slice(&value.to_be_bytes());
        assert_eq!(decode(&bad).unwrap_err().code, ErrorCode::Unsupported);
    }
    let mut bad = valid;
    bad[54..56].copy_from_slice(&2u16.to_be_bytes());
    assert_eq!(decode(&bad).unwrap_err().code, ErrorCode::Unsupported);
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
