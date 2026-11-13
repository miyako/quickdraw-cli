mod common;
use common::*;
use quickdraw::{
    BitmapFont, Container, ErrorCode, Fidelity, FontBook, FontFamily, FontFamilySelection,
    FontResourceSelection, FontResourceType, FontScript, FontSelection, Limits, RenderOptions,
    VectorFormat, export_vector, list_font_resources, parse, render,
};
use std::{fs, process::Command};

fn selection(kerning: bool) -> FontSelection {
    FontSelection {
        family: Some(FontFamilySelection {
            id: 7,
            script: FontScript::Roman,
            kerning,
        }),
        ..Default::default()
    }
}
fn resources(font: Vec<u8>, family: Vec<u8>) -> Vec<u8> {
    font_resources(&[(*b"NFNT", -128, font), (*b"FOND", 7, family)])
}
fn bind(data: &[u8], selection: &FontSelection) -> quickdraw::Result<BitmapFont> {
    BitmapFont::from_container(data, Container::Auto, 3, 0, selection, &Limits::default())
}
fn options(font: BitmapFont) -> RenderOptions {
    let mut fonts = FontBook::default();
    fonts.insert(7, 3, 0, font, &Limits::default()).unwrap();
    RenderOptions {
        fonts,
        fidelity: Fidelity::Approximate,
        ..Default::default()
    }
}
fn picture(fractional: bool, separate: bool) -> quickdraw::Picture {
    let mut p = Pict::new(2, 16, 8);
    p.op(3, &word(7)).op(0xd, &word(3));
    if fractional {
        p.op(0x2e, &words(&[4, 0, 256]));
    }
    if separate {
        p.op(0x28, &[0, 3, 0, 0, 1, b'A']).op(0x29, &[4, 1, b'B']);
    } else {
        p.op(0x28, &[0, 3, 0, 0, 2, b'A', b'B']);
    }
    p.op(0x21, &words(&[3, 12]));
    parse(&p.finish(), Container::Raw, &Limits::default()).unwrap()
}
fn pixel(image: &quickdraw::Image, x: usize, y: usize) -> &[u8] {
    &image.rgb()[(y * 16 + x) * 3..(y * 16 + x) * 3 + 3]
}

#[test]
fn inventories_and_exact_font_selection_work_in_all_resource_containers() {
    let fork = font_resources(&[(*b"FONT", -128, nfnt()), (*b"FOND", 7, fond())]);
    for data in [
        fork.clone(),
        macbinary(&[], &fork, 129),
        appledouble(&fork),
        applesingle(&[], &fork),
    ] {
        let inventory = list_font_resources(&data, Container::Auto, &Limits::default()).unwrap();
        assert_eq!(inventory.resources.len(), 2);
        for entry in &inventory.resources {
            let expected = if entry.resource_type == FontResourceType::Fond {
                fond()
            } else {
                nfnt()
            };
            assert_eq!(&data[entry.offset..entry.offset + entry.length], expected);
        }
        let explicit = FontSelection {
            resource: Some(FontResourceSelection {
                resource_type: FontResourceType::Font,
                id: -128,
            }),
            ..Default::default()
        };
        let actual = render(
            &picture(false, false),
            &options(bind(&data, &explicit).unwrap()),
        )
        .unwrap();
        let baseline = render(
            &picture(false, false),
            &options(BitmapFont::parse(&nfnt(), &Limits::default()).unwrap()),
        )
        .unwrap();
        assert_eq!(actual.image.rgb(), baseline.image.rgb());
        assert!(bind(&data, &selection(false)).is_ok());
    }
}

#[test]
fn fond_width_precedence_disabled_flag_and_pair_kerning_have_observable_pixels() {
    let data = resources(nfnt(), fond());
    for (fractional, kerning, b_left, pen) in [
        (false, false, 5, 7),
        (true, false, 3, 5),
        (false, true, 4, 6),
        (true, true, 3, 5),
    ] {
        let rendered = render(
            &picture(fractional, false),
            &options(bind(&data, &selection(kerning)).unwrap()),
        )
        .unwrap();
        assert_eq!(pixel(&rendered.image, b_left, 0), &[0; 3]);
        assert_eq!(pixel(&rendered.image, b_left + 2, 0), &[255; 3]);
        assert_eq!(pixel(&rendered.image, pen, 3), &[0; 3]);
        assert_eq!(pixel(&rendered.image, pen - 1, 3), &[255; 3]);
    }
    let mut family = fond();
    family[..2].copy_from_slice(&0x4002u16.to_be_bytes());
    let rendered = render(
        &picture(true, false),
        &options(bind(&resources(nfnt(), family), &selection(false)).unwrap()),
    )
    .unwrap();
    assert_eq!(pixel(&rendered.image, 5, 0), &[0; 3]); // disabled FOND falls back to integer NFNT
    let mut font = nfnt();
    font[1] |= 2;
    font.extend(words(&[0x0380, 0x0300, 0x0200])); // A=3.5 overrides family A=2.25
    let rendered = render(
        &picture(true, false),
        &options(bind(&resources(font, fond()), &selection(false)).unwrap()),
    )
    .unwrap();
    assert_eq!(pixel(&rendered.image, 5, 0), &[0; 3]);
    assert_eq!(pixel(&rendered.image, 7, 3), &[0; 3]);
}

#[test]
fn opt_in_kerning_scales_with_ratio_but_does_not_cross_positioned_runs() {
    let data = resources(nfnt(), fond());
    let with = options(bind(&data, &selection(true)).unwrap());
    let without = options(bind(&data, &selection(false)).unwrap());
    let p = picture(false, true);
    assert_eq!(
        render(&p, &with).unwrap().image.rgb(),
        render(&p, &without).unwrap().image.rgb()
    );
    let mut strict = with.clone();
    strict.fidelity = Fidelity::Strict;
    assert_eq!(
        render(&p, &strict).unwrap_err().code,
        ErrorCode::Unsupported
    );
    let mut p = Pict::new(2, 16, 8);
    p.op(3, &word(7))
        .op(0xd, &word(3))
        .op(0x10, &words(&[1, 2, 1, 1]));
    p.op(0x28, &[0, 3, 0, 0, 2, b'A', b'B']);
    let p = parse(&p.finish(), Container::Raw, &Limits::default()).unwrap();
    let rendered = render(&p, &with).unwrap();
    assert_eq!(pixel(&rendered.image, 8, 0), &[255; 3]);
    assert_eq!(pixel(&rendered.image, 9, 0), &[0; 3]); // (4-.75+1)*2 rounds to 9
    for format in [VectorFormat::Svg, VectorFormat::Pdf] {
        let vector = export_vector(&p, &with, 72., format).unwrap();
        assert!(
            vector
                .diagnostics
                .iter()
                .any(|d| d.message.contains("FOND pair kerning"))
        );
    }
}

#[test]
fn families_require_exact_associations_and_explicit_type_for_ambiguous_ids() {
    let both = font_resources(&[
        (*b"FONT", -128, nfnt()),
        (*b"NFNT", -128, nfnt()),
        (*b"FOND", 7, fond()),
    ]);
    assert_eq!(
        bind(&both, &selection(false)).unwrap_err().code,
        ErrorCode::InvalidArguments
    );
    let mut selected = selection(false);
    selected.resource = Some(FontResourceSelection {
        resource_type: FontResourceType::Nfnt,
        id: -128,
    });
    assert!(bind(&both, &selected).is_ok());
    selected.resource.as_mut().unwrap().id = 9;
    assert!(
        bind(&both, &selected)
            .unwrap_err()
            .message
            .contains("conflicts")
    );
    let family = FontFamily::parse(&fond(), &Limits::default()).unwrap();
    for (size, face) in [(0, 0), (4, 0), (3, 1), (3, 128)] {
        assert_eq!(
            family.resource_id(size, face).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
    assert_eq!(
        bind(&both, &FontSelection::default()).unwrap_err().code,
        ErrorCode::InvalidArguments
    );
    let no_font = font_resources(&[(*b"FOND", 7, fond())]);
    assert!(
        bind(&no_font, &selection(false))
            .unwrap_err()
            .message
            .contains("missing")
    );
    let font = bind(&resources(nfnt(), fond()), &selection(false)).unwrap();
    let mut book = FontBook::default();
    assert_eq!(
        book.insert(7, 4, 0, font.clone(), &Limits::default())
            .unwrap_err()
            .code,
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        book.insert(7, 3, 1, font.clone(), &Limits::default())
            .unwrap_err()
            .code,
        ErrorCode::InvalidArguments
    );
    assert_eq!(
        book.insert(
            7,
            3,
            0,
            font,
            &Limits {
                max_decoded_bytes: 100,
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn fond_bounds_directory_duplicates_terminators_and_limits_are_checked() {
    let valid = fond();
    for end in 0..valid.len() {
        assert!(
            FontFamily::parse(&valid[..end], &Limits::default()).is_err(),
            "prefix {end}"
        );
    }
    for (at, value) in [
        (16, 0u16),
        (18, 58),
        (22, 60),
        (70, 1),
        (52, 0xfffe),
        (76, 0xffff),
    ] {
        let mut bad = valid.clone();
        bad[at..at + 2].copy_from_slice(&value.to_be_bytes());
        if at == 16 {
            bad[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        }
        assert!(
            FontFamily::parse(&bad, &Limits::default()).is_err(),
            "offset {at}"
        );
    }
    let mut duplicate = valid.clone();
    duplicate[76..78].copy_from_slice(&2u16.to_be_bytes());
    duplicate.extend_from_slice(&valid[78..82]);
    assert!(
        FontFamily::parse(&duplicate, &Limits::default())
            .unwrap_err()
            .message
            .contains("duplicate")
    );
    let mut directory = valid[..60].to_vec();
    directory.extend(word(1));
    directory.extend(10u32.to_be_bytes());
    directory.extend(22u32.to_be_bytes());
    directory.extend_from_slice(&valid[60..]);
    directory[16..20].copy_from_slice(&70u32.to_be_bytes());
    directory[20..24].copy_from_slice(&82u32.to_be_bytes());
    assert!(bind(&resources(nfnt(), directory.clone()), &selection(true)).is_ok());
    directory[62..66].copy_from_slice(&2u32.to_be_bytes());
    assert!(FontFamily::parse(&directory, &Limits::default()).is_err());
    for limits in [
        Limits {
            max_operations: 2,
            ..Default::default()
        },
        Limits {
            max_decoded_bytes: 90,
            ..Default::default()
        },
        Limits {
            max_work: 10,
            ..Default::default()
        },
    ] {
        assert_eq!(
            FontFamily::parse(&valid, &limits).unwrap_err().code,
            ErrorCode::ResourceLimit
        );
    }
    let mut legacy = fond();
    legacy[50..52].copy_from_slice(&1u16.to_be_bytes());
    assert!(bind(&resources(nfnt(), legacy.clone()), &selection(false)).is_ok());
    assert_eq!(
        bind(&resources(nfnt(), legacy), &selection(true))
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}

#[test]
fn cli_font_inventory_binding_script_errors_and_container_source_protection() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("font.as");
    let map = dir.path().join("fonts.json");
    let input = dir.path().join("text.pict");
    let output = dir.path().join("text.png");
    let source_bytes = applesingle(&[], &resources(nfnt(), fond()));
    fs::write(&source, &source_bytes).unwrap();
    let inventory = Command::new(env!("CARGO_BIN_EXE_quickdraw"))
        .arg("list-fonts")
        .arg("--input")
        .arg(&source)
        .output()
        .unwrap();
    assert!(inventory.status.success());
    let json: serde_json::Value = serde_json::from_slice(&inventory.stdout).unwrap();
    assert_eq!(json["protocol"], 1);
    assert_eq!(json["inventory"]["container"], "applesingle");
    assert_eq!(json["inventory"]["resources"][1]["resource_type"], "NFNT");
    let mut p = Pict::new(2, 16, 8);
    p.op(3, &word(7))
        .op(0xd, &word(3))
        .op(0x28, &[0, 3, 0, 0, 2, b'A', b'B']);
    fs::write(&input, p.finish()).unwrap();
    let mut binding = serde_json::json!({"schema":1,"fonts":[{"id":7,"size":3,"path":"font.as","container":"auto","family":{"id":7,"script":"roman","kerning":true}}]});
    let run = |target: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_quickdraw"))
            .arg("convert")
            .arg("--input")
            .arg(&input)
            .arg("--output")
            .arg(target)
            .arg("--font-map")
            .arg(&map)
            .args([
                "--format",
                "png",
                "--fidelity",
                "approximate",
                "--overwrite",
            ])
            .output()
            .unwrap()
    };
    fs::write(&map, binding.to_string()).unwrap();
    let result = run(&output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let before = fs::read(&output).unwrap();
    assert_eq!(run(&source).status.code(), Some(3));
    let alias = dir.path().join("alias.as");
    fs::hard_link(&source, &alias).unwrap();
    assert_eq!(run(&alias).status.code(), Some(3));
    assert_eq!(fs::read(&source).unwrap(), source_bytes);
    for script in [serde_json::json!("japanese"), serde_json::Value::Null] {
        binding["fonts"][0]["family"]["script"] = script;
        fs::write(&map, binding.to_string()).unwrap();
        assert_eq!(run(&output).status.code(), Some(2));
        assert_eq!(fs::read(&output).unwrap(), before);
    }
}
