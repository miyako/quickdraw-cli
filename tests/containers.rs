mod common;
use common::*;
use quickdraw::{
    Container, ErrorCode, Limits, PictureSelector, RenderOptions, list_pictures, parse,
    parse_selected, render,
};
use std::{fs, process::Command};

fn pict(paint: bool) -> Vec<u8> {
    let mut p = Pict::new(2, 2, 1);
    if paint {
        p.op(0x31, &rect(0, 0, 1, 2));
    }
    p.finish()
}
#[test]
fn resource_inventory_and_signed_selection_retain_global_opcode_offsets() {
    let data = resource_fork(&[(-128, pict(false)), (256, pict(true))]);
    let inventory = list_pictures(&data, Container::Auto, &Limits::default()).unwrap();
    assert_eq!(inventory.container, Container::ResourceFork);
    assert_eq!(inventory.pictures.len(), 2);
    assert_eq!(inventory.pictures[0].name_hex.as_deref(), Some("74657374"));
    assert_eq!(
        parse(&data, Container::Auto, &Limits::default())
            .unwrap_err()
            .code,
        ErrorCode::InvalidArguments
    );
    let p = parse_selected(
        &data,
        Container::Auto,
        PictureSelector::Resource(256),
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(
        p.inspect().operations[0].offset,
        inventory.pictures[1].offset + 40
    );
    assert_eq!(p.resource_id(), Some(256));
    assert_eq!(
        render(&p, &RenderOptions::default()).unwrap().image.rgb(),
        [0; 6]
    );
    assert_eq!(
        parse_selected(
            &data,
            Container::Auto,
            PictureSelector::Resource(0),
            &Limits::default()
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidArguments
    );
}
#[test]
fn macbinary_versions_require_unambiguous_data_vs_resource_selection() {
    let rf = resource_fork(&[(5, pict(true))]);
    let mut file = vec![0; 512];
    file.extend(pict(false));
    for version in [0, 129, 130] {
        let data = macbinary(&file, &rf, version);
        let inventory = list_pictures(&data, Container::Auto, &Limits::default()).unwrap();
        assert_eq!(inventory.pictures.len(), 2);
        assert!(parse(&data, Container::Auto, &Limits::default()).is_err());
        let p = parse_selected(
            &data,
            Container::Auto,
            PictureSelector::DataFork,
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(p.inspect().source_offset, 640);
        assert_eq!(
            render(&p, &RenderOptions::default()).unwrap().image.rgb(),
            [255; 6]
        );
        let p = parse_selected(
            &data,
            Container::Auto,
            PictureSelector::Resource(5),
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(
            render(&p, &RenderOptions::default()).unwrap().image.rgb(),
            [0; 6]
        );
    }
}
#[test]
fn appledouble_extracts_only_its_embedded_resource_fork() {
    let data = appledouble(&resource_fork(&[(9, pict(true))]));
    let p = parse(&data, Container::Auto, &Limits::default()).unwrap();
    assert_eq!(p.resource_id(), Some(9));
    assert_eq!(p.inspect().source_offset, 38 + 260);
    assert_eq!(
        parse_selected(
            &data,
            Container::AppleDouble,
            PictureSelector::DataFork,
            &Limits::default()
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidArguments
    );
}
#[test]
fn all_container_truncations_fail_and_do_not_panic() {
    let rf = resource_fork(&[(1, pict(true))]);
    for (kind, data) in [
        (Container::ResourceFork, rf.clone()),
        (Container::MacBinary, macbinary(&[], &rf, 129)),
        (Container::AppleDouble, appledouble(&rf)),
        (Container::AppleSingle, applesingle(&[], &rf)),
    ] {
        for end in 0..data.len() {
            assert!(
                parse(&data[..end], kind, &Limits::default()).is_err(),
                "{kind:?} prefix {end}"
            );
        }
    }
}
#[test]
fn applesingle_shares_bounds_and_requires_explicit_fork_selection() {
    let data = applesingle(&pict(false), &resource_fork(&[(-7, pict(true))]));
    let inventory = list_pictures(&data, Container::Auto, &Limits::default()).unwrap();
    assert_eq!(inventory.container, Container::AppleSingle);
    assert_eq!(inventory.pictures.len(), 2);
    assert_eq!(
        parse(&data, Container::Auto, &Limits::default())
            .unwrap_err()
            .code,
        ErrorCode::InvalidArguments
    );
    for (selector, expected) in [
        (PictureSelector::DataFork, [255; 6]),
        (PictureSelector::Resource(-7), [0; 6]),
    ] {
        let picture = parse_selected(&data, Container::Auto, selector, &Limits::default()).unwrap();
        assert_eq!(
            render(&picture, &RenderOptions::default())
                .unwrap()
                .image
                .rgb(),
            expected
        );
    }
    for (at, value) in [(4, 0x10000u32), (38, 1), (30, 0), (34, u32::MAX), (42, 50)] {
        let mut bad = data.clone();
        bad[at..at + 4].copy_from_slice(&value.to_be_bytes());
        assert!(
            list_pictures(&bad, Container::AppleSingle, &Limits::default()).is_err(),
            "offset {at}"
        );
    }
    let mut file = vec![0; 512];
    file.extend(pict(true));
    let single = applesingle(&file, &[]);
    let picture = parse(&single, Container::Auto, &Limits::default()).unwrap();
    assert_eq!(picture.inspect().source_offset, 562);
}
#[test]
fn resource_overlap_duplicates_names_and_offsets_are_validated() {
    let rf = resource_fork(&[(1, pict(true)), (2, pict(false))]);
    let map = u32::from_be_bytes(rf[4..8].try_into().unwrap()) as usize;
    for (at, value) in [
        (map + 50, 1u16),
        (map + 40, 0x7fff),
        (map + 36, 0),
        (map + 26, 28),
    ] {
        let mut bad = rf.clone();
        bad[at..at + 2].copy_from_slice(&value.to_be_bytes());
        assert!(parse(&bad, Container::ResourceFork, &Limits::default()).is_err());
    }
    let mut bad = rf.clone();
    bad[map + 55..map + 58].copy_from_slice(&[0xff; 3]);
    assert_eq!(
        parse(&bad, Container::ResourceFork, &Limits::default())
            .unwrap_err()
            .code,
        ErrorCode::Malformed
    );
    let mut duplicate = resource_fork(&[(7, pict(true)), (7, pict(false))]);
    assert!(list_pictures(&duplicate, Container::ResourceFork, &Limits::default()).is_err());
    duplicate[0..4].copy_from_slice(&0xfffffff0u32.to_be_bytes());
    assert!(list_pictures(&duplicate, Container::ResourceFork, &Limits::default()).is_err());
}
#[test]
fn macbinary_crc_and_padding_are_not_ignored() {
    let valid = macbinary(&pict(true), &[], 129);
    let mut corrupt = valid.clone();
    corrupt[69] ^= 1;
    assert!(
        parse(&corrupt, Container::MacBinary, &Limits::default())
            .unwrap_err()
            .message
            .contains("CRC")
    );
    let mut corrupt = valid;
    *corrupt.last_mut().unwrap() = 1;
    assert!(
        parse(&corrupt, Container::MacBinary, &Limits::default())
            .unwrap_err()
            .message
            .contains("padding")
    );
}
#[test]
fn appledouble_version_entry_ranges_and_duplicate_ids_fail() {
    let valid = appledouble(&resource_fork(&[(7, pict(true))]));
    for (at, word) in [(4, 0x10000u32), (26, 1), (30, u32::MAX), (34, u32::MAX)] {
        let mut bad = valid.clone();
        bad[at..at + 4].copy_from_slice(&word.to_be_bytes());
        assert!(parse(&bad, Container::AppleDouble, &Limits::default()).is_err());
    }
    let mut bad = valid;
    bad[24..26].copy_from_slice(&2u16.to_be_bytes());
    let entry = bad[26..38].to_vec();
    bad[38..50].copy_from_slice(&entry);
    assert!(list_pictures(&bad, Container::AppleDouble, &Limits::default()).is_err());
}
#[test]
fn limits_cover_container_tables_before_parsing_selected_picture() {
    let rf = resource_fork(&[(1, pict(true)), (2, pict(false))]);
    let limits = Limits {
        max_operations: 1,
        ..Default::default()
    };
    assert_eq!(
        parse_selected(
            &rf,
            Container::ResourceFork,
            PictureSelector::Resource(1),
            &limits
        )
        .unwrap_err()
        .code,
        ErrorCode::ResourceLimit
    );
    let limits = Limits {
        max_work: 100,
        ..Default::default()
    };
    assert_eq!(
        list_pictures(&rf, Container::Auto, &limits)
            .unwrap_err()
            .code,
        ErrorCode::ResourceLimit
    );
}
#[test]
fn cli_lists_then_selects_without_writing_any_embedded_names() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("two.rsrc");
    fs::write(
        &input,
        resource_fork(&[(-128, pict(true)), (4, pict(false))]),
    )
    .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_quickdraw"))
            .args(args)
            .arg("--input")
            .arg(&input)
            .output()
            .unwrap()
    };
    let list = run(&["list-pictures"]);
    assert!(list.status.success());
    let info: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(info["inventory"]["pictures"][0]["resource_id"], -128);
    assert_eq!(run(&["inspect"]).status.code(), Some(2));
    let inspect = run(&["inspect", "--resource-id", "-128"]);
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
