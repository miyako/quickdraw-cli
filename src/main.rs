use clap::{Args, Parser, Subcommand, ValueEnum};
use quickdraw::{Container, Error, ErrorCode, Fidelity, Limits, RenderOptions, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    name = "quickdraw",
    about = "Convert a documented subset of QuickDraw PICT safely. Results and errors use JSON protocol 1."
)]
struct Cli {
    /// Print JSON protocol and program version.
    #[arg(long, exclusive = true)]
    version: bool,
    #[command(subcommand)]
    command: Option<Action>,
}
#[derive(Subcommand)]
enum Action {
    /// Convert to opaque PNG or explicitly raster-backed SVG.
    Convert(Convert),
    /// Parse and inspect opcodes/comments; does not promise renderability.
    Inspect(Input),
    /// Describe the supported subset and versioned machine contract.
    Capabilities,
}
#[derive(Args)]
struct Input {
    /// Path to a single PICT resource or a 512-byte-header PICT file.
    #[arg(long)]
    input: PathBuf,
    #[arg(long, value_enum, default_value = "auto")]
    container: Container,
    #[command(flatten)]
    limits: ResourceLimits,
}
#[derive(Args)]
struct ResourceLimits {
    #[arg(long, default_value_t = 64 * 1024 * 1024)]
    max_input_bytes: usize,
    #[arg(long, default_value_t = 16_000_000)]
    max_pixels: usize,
    #[arg(long, default_value_t = 128 * 1024 * 1024)]
    max_decoded_bytes: usize,
    #[arg(long, default_value_t = 100_000)]
    max_operations: usize,
    #[arg(long, default_value_t = 100_000_000)]
    max_work: u64,
}
impl ResourceLimits {
    fn value(&self) -> Limits {
        Limits {
            max_input_bytes: self.max_input_bytes,
            max_pixels: self.max_pixels,
            max_decoded_bytes: self.max_decoded_bytes,
            max_operations: self.max_operations,
            max_work: self.max_work,
        }
    }
}
#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
enum Format {
    Png,
    Svg,
}
#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
enum VectorPolicy {
    Reject,
    Raster,
}
#[derive(Args)]
struct Convert {
    #[command(flatten)]
    input: Input,
    #[arg(long)]
    output: PathBuf,
    /// Explicit format, otherwise inferred from .png or .svg output suffix.
    #[arg(long, value_enum)]
    format: Option<Format>,
    /// Strict rejects known approximate geometry/comments; neither mode is an emulator.
    #[arg(long, value_enum, default_value = "strict")]
    fidelity: Fidelity,
    /// SVG requires raster: embed the entire composited image.
    #[arg(long, value_enum, default_value = "reject")]
    vector_policy: VectorPolicy,
    /// Final pixel width; with height omitted, preserve aspect ratio.
    #[arg(long)]
    width: Option<u32>,
    /// Final pixel height; with width omitted, preserve aspect ratio.
    #[arg(long)]
    height: Option<u32>,
    /// Physical output resolution; never changes pixel dimensions.
    #[arg(long, default_value_t = 72.0)]
    dpi: f64,
    /// Initial opaque canvas color, RRGGBB (independent of QuickDraw BkColor).
    #[arg(long, default_value = "ffffff", value_parser = parse_color)]
    background: [u8; 3],
    /// Atomically replace an existing regular output file, never the input or its aliases.
    #[arg(long)]
    overwrite: bool,
}

fn parse_color(text: &str) -> std::result::Result<[u8; 3], String> {
    if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("background must be six ASCII hexadecimal digits (RRGGBB)".into());
    }
    let value = u32::from_str_radix(text, 16).map_err(|e| e.to_string())?;
    Ok([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}
fn io_error(error: std::io::Error) -> Error {
    Error::new(ErrorCode::Io, error.to_string())
}

fn read_input(input: &Input) -> Result<Vec<u8>> {
    require_utf8(&input.input)?;
    let limits = input.limits.value();
    limits.validate()?;
    let mut file = File::open(&input.input).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() {
        return Err(Error::new(ErrorCode::Io, "input must be a regular file"));
    }
    if metadata.len() > limits.max_input_bytes as u64 {
        return Err(Error::new(
            ErrorCode::ResourceLimit,
            "input byte limit exceeded",
        ));
    }
    let bound = (limits.max_input_bytes as u64)
        .checked_add(1)
        .ok_or_else(|| Error::new(ErrorCode::InvalidArguments, "input byte limit too large"))?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(bound)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > limits.max_input_bytes {
        return Err(Error::new(
            ErrorCode::ResourceLimit,
            "input grew beyond byte limit",
        ));
    }
    Ok(bytes)
}

fn output_path(input: &Path, output: &Path, overwrite: bool) -> Result<PathBuf> {
    require_utf8(input)?;
    require_utf8(output)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = output
        .file_name()
        .ok_or_else(|| Error::new(ErrorCode::InvalidArguments, "output needs a filename"))?;
    let target = parent.canonicalize().map_err(io_error)?.join(name);
    if input.canonicalize().map_err(io_error)? == target {
        return Err(Error::new(
            ErrorCode::OutputConflict,
            "refusing to overwrite the source picture",
        ));
    }
    match fs::symlink_metadata(&target) {
        Ok(meta) => {
            if meta.is_symlink() || !meta.is_file() {
                return Err(Error::new(
                    ErrorCode::OutputConflict,
                    "output must not be a symlink or nonregular file",
                ));
            }
            if same_file::is_same_file(input, &target).map_err(io_error)? {
                return Err(Error::new(
                    ErrorCode::OutputConflict,
                    "output is an alias of the input",
                ));
            }
            if !overwrite {
                return Err(Error::new(
                    ErrorCode::OutputExists,
                    "output exists; use --overwrite for a different regular file",
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_error(e)),
    }
    Ok(target)
}

fn require_utf8(path: &Path) -> Result<()> {
    if path.to_str().is_none() {
        return Err(Error::new(
            ErrorCode::InvalidArguments,
            "paths must be valid UTF-8 for JSON protocol 1",
        ));
    }
    Ok(())
}

fn write_output(path: &Path, bytes: &[u8], overwrite: bool) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new(ErrorCode::Io, "output has no parent"))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    temp.write_all(bytes).map_err(io_error)?;
    temp.as_file().sync_all().map_err(io_error)?;
    let result = if overwrite {
        temp.persist(path)
    } else {
        temp.persist_noclobber(path)
    };
    result.map_err(|e| {
        if e.error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::new(
                ErrorCode::OutputExists,
                "output was created concurrently; no file replaced",
            )
        } else {
            io_error(e.error)
        }
    })?;
    Ok(())
}

fn capabilities() -> Value {
    json!({
        "protocol": 1, "version": env!("CARGO_PKG_VERSION"),
        "library": "quickdraw", "binary": "quickdraw",
        "formats": ["png", "svg"],
        "svg": "whole_picture_raster_only_requires_explicit_opt_in",
        "containers": ["auto", "raw", "file"],
        "pict_versions": [1, 2],
        "raster_profile": "portable_rgb_v1",
        "fidelity": {
            "strict": "reject known approximations, not a pixel-identity guarantee",
            "approximate": "allow portable curve/polygon/diagonal coverage and ignored printer comments with diagnostics",
            "classic_oracle_validated": false
        },
        "supported": [
            "rectangles", "rectangular_pen", "8x8_patterns", "rgb_colors",
            "rectangular_and_transition_regions", "ordered_boolean_compositing",
            "monochrome_bitmaps", "indexed_1_2_4_8_bit_pixmaps",
            "rgb555_and_rgb32_pixmaps", "packbits_byte_and_word_runs",
            "source_destination_bitmap_rectangles", "bitmap_region_masks"
        ],
        "approximate_only": ["ovals", "polygons", "diagonal_lines", "complex_region_outlines", "printer_comments", "color_bitmap_boolean_modes"],
        "unsupported": [
            "text_and_fonts", "origin_changes", "arcs", "rounded_rectangles",
            "pixel_patterns", "highlight_mode", "arithmetic_transfer_modes",
            "quicktime_codecs", "resource_fork_maps", "macbinary", "appledouble",
            "editable_vector_output", "pdf", "classic_screen_pixel_identity"
        ],
        "default_limits": Limits::default(),
        "exit_codes": {"success":0,"arguments":2,"io_or_output_conflict":3,"malformed":4,"unsupported":5,"resource_limit":6,"encoding":7},
        "warnings": "success is always qualified by diagnostics; callers must retain them"
    })
}

fn execute(cli: Cli) -> Result<Value> {
    if cli.version {
        if cli.command.is_some() {
            return Err(Error::new(
                ErrorCode::InvalidArguments,
                "--version cannot be combined with a subcommand",
            ));
        }
        return Ok(json!({"protocol":1,"version":env!("CARGO_PKG_VERSION")}));
    }
    match cli.command.ok_or_else(|| {
        Error::new(
            ErrorCode::InvalidArguments,
            "expected convert, inspect, capabilities, --help or --version",
        )
    })? {
        Action::Capabilities => Ok(capabilities()),
        Action::Inspect(input) => {
            let bytes = read_input(&input)?;
            let picture = quickdraw::parse(&bytes, input.container, &input.limits.value())?;
            Ok(json!({"protocol":1,"inspection":picture.inspect()}))
        }
        Action::Convert(args) => {
            let format = args
                .format
                .or_else(|| {
                    match args
                        .output
                        .extension()?
                        .to_str()?
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "png" => Some(Format::Png),
                        "svg" => Some(Format::Svg),
                        _ => None,
                    }
                })
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::InvalidArguments,
                        "select --format png|svg or use a .png/.svg suffix",
                    )
                })?;
            if format == Format::Svg && args.vector_policy != VectorPolicy::Raster {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "SVG is raster-backed only; explicitly select --vector-policy raster",
                ));
            }
            if format != Format::Svg && args.vector_policy == VectorPolicy::Raster {
                return Err(Error::new(
                    ErrorCode::InvalidArguments,
                    "--vector-policy raster applies only to SVG",
                ));
            }
            if !args.dpi.is_finite() || args.dpi <= 0.0 || args.dpi > 100_000.0 {
                return Err(Error::new(
                    ErrorCode::InvalidArguments,
                    "DPI must be finite and in (0, 100000]",
                ));
            }
            let target = output_path(&args.input.input, &args.output, args.overwrite)?;
            let bytes = read_input(&args.input)?;
            let options = RenderOptions {
                fidelity: args.fidelity,
                background: args.background,
                width: args.width,
                height: args.height,
                limits: args.input.limits.value(),
            };
            let picture = quickdraw::parse(&bytes, args.input.container, &options.limits)?;
            let mut rendered = quickdraw::render(&picture, &options)?;
            let encoded = match format {
                Format::Png => quickdraw::encode_png(&rendered.image, args.dpi)?,
                Format::Svg => {
                    rendered.diagnostics.push(quickdraw::Diagnostic {
                        code: "raster_fallback",
                        message: "Entire composited picture embedded as PNG; no editable vectors."
                            .into(),
                        offset: None,
                    });
                    quickdraw::encode_svg(&rendered.image, args.dpi)?
                }
            };
            // Recheck aliases immediately before committing; rename never follows a target symlink.
            output_path(&args.input.input, &args.output, args.overwrite)?;
            write_output(&target, &encoded, args.overwrite)?;
            Ok(json!({
                "protocol":1, "output":args.output, "format":if format == Format::Png {"png"} else {"svg"},
                "width":rendered.image.width(), "height":rendered.image.height(), "dpi":args.dpi,
                "bytes":encoded.len(), "sha256":format!("{:x}", Sha256::digest(&encoded)),
                "input_sha256":format!("{:x}", Sha256::digest(&bytes)),
                "raster_profile":"portable_rgb_v1", "diagnostics":rendered.diagnostics
            }))
        }
    }
}

fn main() -> ExitCode {
    let result = match Cli::try_parse() {
        Ok(cli) => execute(cli),
        Err(error) if error.kind() == clap::error::ErrorKind::DisplayHelp => {
            return match std::io::stdout()
                .lock()
                .write_all(error.to_string().as_bytes())
            {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => ExitCode::from(3),
            };
        }
        Err(error) => Err(Error::new(ErrorCode::InvalidArguments, error.to_string())),
    };
    match result {
        Ok(value) => {
            let text = format!("{value}\n");
            if let Err(error) = std::io::stdout().lock().write_all(text.as_bytes()) {
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "{}",
                    json!({"protocol":1,"error":io_error(error)})
                );
                ExitCode::from(3)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            let code = error.exit_code();
            let _ = writeln!(
                std::io::stderr().lock(),
                "{}",
                json!({"protocol":1,"error":error})
            );
            ExitCode::from(code)
        }
    }
}
