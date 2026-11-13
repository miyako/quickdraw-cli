mod common;
use common::*;
use quickdraw::{
    Container, ErrorCode, Fidelity, Limits, RenderOptions, VectorFormat, export_vector, parse,
    render,
};
use serde_json::Value;

fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("fixtures/quicktime-stills.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
fn bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}
fn pict(codec: &[u8; 4], width: u16, height: u16, data: &[u8], mode: u16) -> Vec<u8> {
    let mut qt = quicktime(codec, width, height, data, identity_matrix(), &[]);
    qt[54..56].copy_from_slice(&mode.to_be_bytes());
    let mut p = Pict::new(2, width as i16, height as i16);
    p.op(0x8200, &qt);
    p.finish()
}
fn sample(name: &str) -> Vec<u8> {
    let c = cases().into_iter().find(|c| c["name"] == name).unwrap();
    bytes(c["packet"].as_str().unwrap())
}
fn error(codec: &[u8; 4], data: &[u8]) -> ErrorCode {
    parse(
        &pict(codec, 3, 2, data, 0),
        Container::Raw,
        &Limits::default(),
    )
    .unwrap_err()
    .code
}
#[test]
fn independent_still_fixtures_decode_pixels_and_reject_ambiguous_images() {
    for c in cases() {
        let codec = c["codec"].as_str().unwrap().as_bytes().try_into().unwrap();
        let data = bytes(c["packet"].as_str().unwrap());
        if c.get("error").is_some() {
            assert_eq!(error(codec, &data), ErrorCode::Unsupported, "{}", c["name"]);
            continue;
        }
        let width = c["width"].as_u64().unwrap() as u16;
        let height = c["height"].as_u64().unwrap() as u16;
        let rgba = bytes(c["rgba"].as_str().unwrap());
        let mode = if rgba.chunks_exact(4).any(|p| p[3] != 255) {
            256
        } else {
            0
        };
        let pic = parse(
            &pict(codec, width, height, &data, mode),
            Container::Raw,
            &Limits::default(),
        )
        .unwrap_or_else(|e| panic!("{}: {e:?}", c["name"]));
        let opts = RenderOptions {
            fidelity: Fidelity::Approximate,
            ..Default::default()
        };
        let actual = render(&pic, &opts).unwrap().image.rgb().to_vec();
        let expected: Vec<u8> = rgba
            .chunks_exact(4)
            .flat_map(|p| {
                p[..3].iter().map(move |&v| {
                    ((u32::from(v) * u32::from(p[3]) + 255 * (255 - u32::from(p[3])) + 127) / 255)
                        as u8
                })
            })
            .collect();
        assert_eq!(actual, expected, "{}", c["name"]);
        for format in [VectorFormat::Svg, VectorFormat::Pdf] {
            assert!(
                export_vector(&pic, &opts, 72., format).is_ok(),
                "{}",
                c["name"]
            );
        }
    }
}

fn tag(data: &[u8], id: u16) -> usize {
    assert_eq!(&data[..2], b"II");
    let at = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(data[at..at + 2].try_into().unwrap()) as usize;
    (0..count)
        .map(|i| at + 2 + i * 12)
        .find(|&i| u16::from_le_bytes(data[i..i + 2].try_into().unwrap()) == id)
        .unwrap()
}

#[test]
fn tiff_directory_color_alpha_and_decompression_boundaries_are_explicit() {
    let base = sample("tiff-rgb-raw");
    for (id, value, expected) in [
        (256, 4, ErrorCode::Malformed),
        (262, 5, ErrorCode::Unsupported),
        (259, 7, ErrorCode::Unsupported),
        (278, 0, ErrorCode::Malformed),
    ] {
        let mut data = base.clone();
        let i = tag(&data, id);
        data[i + 8..i + 12].copy_from_slice(&u32::to_le_bytes(value));
        assert_eq!(error(b"tiff", &data), expected);
    }
    let mut data = sample("tiff-rgba-raw");
    let i = tag(&data, 338);
    data[i + 8..i + 10].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(error(b"tiff", &data), ErrorCode::Unsupported);
    let mut data = base.clone();
    let i = tag(&data, 258);
    data[i + 4..i + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        error(b"tiff", &data),
        ErrorCode::Malformed | ErrorCode::ResourceLimit
    ));
    for name in [
        "tiff-rgb-raw",
        "tiff-rgb-packbits",
        "tiff-rgb-tiff_lzw",
        "tiff-rgb-tiff_adobe_deflate",
    ] {
        let mut data = sample(name);
        let i = tag(&data, 279);
        let len = u32::from_le_bytes(data[i + 8..i + 12].try_into().unwrap());
        data[i + 8..i + 12].copy_from_slice(&(len - 1).to_le_bytes());
        assert_eq!(error(b"tiff", &data), ErrorCode::Malformed, "{name}");
        let mut small = sample(name);
        for id in [256, 257] {
            let i = tag(&small, id);
            small[i + 8..i + 12].copy_from_slice(&1u32.to_le_bytes());
        }
        assert_eq!(
            parse(
                &pict(b"tiff", 1, 1, &small, 0),
                Container::Raw,
                &Limits::default()
            )
            .unwrap_err()
            .code,
            ErrorCode::Malformed,
            "excess output: {name}"
        );
    }
    assert_eq!(&sample("tiff-big-endian")[..2], b"MM");
    let multi = sample("tiff-multiple-strips");
    let i = tag(&multi, 273);
    assert!(u32::from_le_bytes(multi[i + 4..i + 8].try_into().unwrap()) > 1);
}

#[test]
fn gif_palette_transparency_frame_extent_and_endings_are_checked() {
    let base = sample("gif-global");
    let descriptor = 13 + 3 * (1usize << ((base[10] & 7) + 1));
    assert_eq!(base[descriptor], 0x2c);
    let mut missing = base.clone();
    missing[10] &= 0x7f;
    missing.drain(13..descriptor);
    assert_eq!(error(b"gif ", &missing), ErrorCode::Unsupported);
    let mut short = base.clone();
    short[10] &= 0xf8;
    short.drain(19..descriptor);
    assert_eq!(error(b"gif ", &short), ErrorCode::Malformed);
    let mut drawing = base.clone();
    drawing.splice(descriptor..descriptor, [0x21, 0x01]);
    assert_eq!(error(b"gif ", &drawing), ErrorCode::Unsupported);
    for (codec, name) in [(b"gif ", "gif-transparent"), (b"tiff", "tiff-rgba-raw")] {
        assert_eq!(error(codec, &sample(name)), ErrorCode::Unsupported);
    }
    let mut data = base.clone();
    data[descriptor + 1] = 1;
    assert_eq!(error(b"gif ", &data), ErrorCode::Unsupported);
    let mut data = base.clone();
    data[12] = 10;
    assert_eq!(error(b"gif ", &data), ErrorCode::Unsupported);
    let mut data = base.clone();
    data.push(0);
    assert_eq!(error(b"gif ", &data), ErrorCode::Malformed);
    // A smaller declared image must not silently discard excess decoded pixels.
    let mut data = base.clone();
    data[6..10].copy_from_slice(&[1, 0, 1, 0]);
    data[descriptor + 5..descriptor + 9].copy_from_slice(&[1, 0, 1, 0]);
    assert!(
        parse(
            &pict(b"gif ", 1, 1, &data, 0),
            Container::Raw,
            &Limits::default()
        )
        .is_err()
    );
    let interlaced = sample("gif-interlaced");
    let i = 13 + 3 * (1usize << ((interlaced[10] & 7) + 1));
    assert_ne!(interlaced[i + 9] & 0x40, 0);
}

#[test]
fn truncation_and_budget_fail_before_unbounded_still_decoding() {
    for (codec, name) in [(b"gif ", "gif-global"), (b"tiff", "tiff-rgb-raw")] {
        let data = sample(name);
        for n in 0..data.len() {
            assert!(
                parse(
                    &pict(codec, 3, 2, &data[..n], 0),
                    Container::Raw,
                    &Limits::default()
                )
                .is_err(),
                "{name} {n}"
            );
        }
        for limits in [
            Limits {
                max_pixels: 5,
                ..Limits::default()
            },
            Limits {
                max_decoded_bytes: 4096,
                ..Limits::default()
            },
            Limits {
                max_operations: 2,
                ..Limits::default()
            },
            Limits {
                max_work: 100,
                ..Limits::default()
            },
        ] {
            assert_eq!(
                parse(&pict(codec, 3, 2, &data, 0), Container::Raw, &limits)
                    .unwrap_err()
                    .code,
                ErrorCode::ResourceLimit
            );
        }
    }
}
