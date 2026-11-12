mod common;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(args: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quickdraw"))
        .args(args)
        .output()
        .unwrap()
}
fn text(args: &[&str]) -> Output {
    let args: Vec<_> = args.iter().map(OsStr::new).collect();
    run(&args)
}
fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["protocol"], 1);
    assert_eq!(output.stdout.iter().filter(|&&c| c == b'\n').count(), 1);
    json
}
fn error(output: &Output, code: i32, kind: &str) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let json: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(json["protocol"], 1);
    assert_eq!(json["error"]["code"], kind);
    json
}
fn fixture(path: &Path) {
    let mut p = Pict::new(2, 2, 2);
    p.op(0x31, &rect(0, 0, 2, 1));
    fs::write(path, p.finish()).unwrap();
}
fn convert(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        OsStr::new("convert"),
        OsStr::new("--input"),
        input.as_os_str(),
        OsStr::new("--output"),
        output.as_os_str(),
    ];
    args.extend(extra.iter().map(OsStr::new));
    run(&args)
}

#[test]
fn version_and_capabilities_are_machine_readable_not_clap_version_text() {
    assert_eq!(
        success(&text(&["--version"])),
        json!({"protocol":1,"version":env!("CARGO_PKG_VERSION")})
    );
    let caps = success(&text(&["capabilities"]));
    assert_eq!(caps["formats"], json!(["png", "svg", "pdf"]));
    assert_eq!(caps["fidelity"]["classic_oracle_validated"], false);
    error(
        &text(&["--version", "capabilities"]),
        2,
        "invalid_arguments",
    );
}

#[test]
fn help_is_text_and_bad_arguments_are_json_errors() {
    let help = text(&["convert", "--help"]);
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--max-work"));
    for args in [
        vec![],
        vec!["unknown"],
        vec!["convert"],
        vec!["convert", "--wrong"],
    ] {
        error(&text(&args), 2, "invalid_arguments");
    }
}

#[test]
fn conversion_writes_valid_png_with_hash_and_warning_no_source_changes() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("picture with spaces.pict");
    let output = dir.path().join("result.png");
    fixture(&input);
    let before = fs::read(&input).unwrap();
    let response = success(&convert(&input, &output, &["--width", "4", "--dpi", "144"]));
    let png = fs::read(&output).unwrap();
    assert_eq!(response["sha256"], format!("{:x}", Sha256::digest(&png)));
    assert_eq!(
        response["input_sha256"],
        format!("{:x}", Sha256::digest(&before))
    );
    assert_eq!(response["width"], 4);
    assert_eq!(response["height"], 4);
    assert_eq!(response["diagnostics"][0]["code"], "unverified_fidelity");
    assert_eq!(fs::read(&input).unwrap(), before);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let second = dir.path().join("second.png");
    success(&convert(&input, &second, &["--width", "4", "--dpi", "144"]));
    assert_eq!(fs::read(second).unwrap(), png);
}

#[test]
fn unicode_paths_survive_json_without_shell_interpretation() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("画像 café ' & $.pict");
    let output = dir.path().join("変換 café.png");
    fixture(&input);
    let result = success(&convert(&input, &output, &[]));
    assert_eq!(result["output"], output.to_str().unwrap());
    assert!(output.exists());
}

#[test]
fn overwrite_is_explicit_and_source_aliases_are_always_forbidden() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.pict");
    let output = dir.path().join("out.png");
    fixture(&input);
    fs::write(&output, b"original").unwrap();
    error(&convert(&input, &output, &[]), 3, "output_exists");
    assert_eq!(fs::read(&output).unwrap(), b"original");
    success(&convert(&input, &output, &["--overwrite"]));
    error(
        &convert(&input, &input, &["--format", "png", "--overwrite"]),
        3,
        "output_conflict",
    );
    let alias = dir.path().join("alias.png");
    fs::hard_link(&input, &alias).unwrap();
    error(
        &convert(&input, &alias, &["--overwrite"]),
        3,
        "output_conflict",
    );
    assert_eq!(fs::read(&input).unwrap(), fs::read(alias).unwrap());
}

#[cfg(unix)]
#[test]
fn symlink_outputs_cannot_redirect_writes() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.pict");
    let output = dir.path().join("out.png");
    fixture(&input);
    std::os::unix::fs::symlink(&input, &output).unwrap();
    error(
        &convert(&input, &output, &["--overwrite"]),
        3,
        "output_conflict",
    );
    assert!(fs::symlink_metadata(output).unwrap().is_symlink());
}

#[test]
fn malformed_and_unsupported_input_leave_existing_output_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.pict");
    let output = dir.path().join("out.png");
    fs::write(&output, b"keep me").unwrap();
    fs::write(&input, b"not a picture").unwrap();
    error(&convert(&input, &output, &["--overwrite"]), 4, "malformed");
    assert_eq!(fs::read(&output).unwrap(), b"keep me");
    let mut p = Pict::new(2, 2, 2);
    p.op(0x28, &[0, 0, 0, 0, 1, b'A']);
    fs::write(&input, p.finish()).unwrap();
    let e = error(
        &convert(
            &input,
            &output,
            &["--overwrite", "--fidelity", "approximate"],
        ),
        5,
        "unsupported",
    );
    assert_eq!(e["error"]["offset"], 40);
    assert_eq!(e["error"]["opcode"], 0x28);
    assert_eq!(fs::read(&output).unwrap(), b"keep me");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn missing_input_unknown_format_invalid_limits_and_invalid_colors_fail() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.pict");
    let output = dir.path().join("out.png");
    error(&convert(&input, &output, &[]), 3, "io");
    fixture(&input);
    error(
        &convert(&input, &dir.path().join("out.webp"), &[]),
        2,
        "invalid_arguments",
    );
    error(
        &convert(&input, &output, &["--max-pixels", "3"]),
        6,
        "resource_limit",
    );
    for args in [
        vec!["--max-pixels", "0"],
        vec!["--background", "notrgb"],
        vec!["--dpi", "NaN"],
        vec!["--width", "0"],
        vec!["--vector-policy", "raster"],
        vec!["--dpi", "0"],
    ] {
        error(&convert(&input, &output, &args), 2, "invalid_arguments");
    }
    assert!(!output.exists());
}

#[test]
fn svg_requires_explicit_fallback_and_embeds_the_complete_composite() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.pict");
    let output = dir.path().join("out.svg");
    let mut p = Pict::new(2, 3, 1);
    p.op(0x31, &rect(0, 0, 1, 2))
        .op(8, &word(10))
        .op(0x31, &rect(0, 1, 1, 3));
    fs::write(&input, p.finish()).unwrap();
    error(&convert(&input, &output, &[]), 5, "unsupported");
    assert!(!output.exists());
    let response = success(&convert(&input, &output, &["--vector-policy", "raster"]));
    assert!(
        response["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["code"] == "raster_fallback")
    );
    let svg = fs::read_to_string(&output).unwrap();
    let encoded = svg
        .split("base64,")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let png = STANDARD.decode(encoded).unwrap();
    let mut reader = png::Decoder::new(png.as_slice()).read_info().unwrap();
    let mut decoded = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut decoded).unwrap();
    assert_eq!(decoded, [0, 0, 0, 255, 255, 255, 0, 0, 0]);
}

#[test]
fn inspect_reports_raw_comment_bytes_even_when_conversion_is_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("comments.pict");
    let mut p = Pict::new(1, 2, 2);
    p.op(0xa1, &[0, 160, 0, 2, 0x80, 0xff]);
    fs::write(&input, p.finish()).unwrap();
    let result = success(&run(&[
        OsStr::new("inspect"),
        OsStr::new("--input"),
        input.as_os_str(),
    ]));
    assert_eq!(result["inspection"]["operations"][0]["data_hex"], "80ff");
    assert_eq!(result["inspection"]["operations"][0]["comment_kind"], 160);
}

#[cfg(unix)]
#[test]
fn non_utf8_paths_fail_cleanly_without_serialization_panic() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.pict");
    fixture(&input);
    let output = dir
        .path()
        .join(std::ffi::OsString::from_vec(b"out\xff.png".to_vec()));
    error(&convert(&input, &output, &[]), 2, "invalid_arguments");
    assert!(!output.exists());
}
