#![recursion_limit = "256"]

use clap::{Args, Parser, Subcommand, ValueEnum};
use quickdraw::{Container, Error, ErrorCode, Fidelity, Limits, RenderOptions, Result};
use serde::Deserialize;
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
    /// Convert to PNG, or explicit preserved-vector/raster-backed SVG or PDF.
    Convert(Convert),
    /// Parse and inspect opcodes/comments; does not promise renderability.
    Inspect(Inspect),
    /// List bounded picture candidates without choosing or rendering one.
    ListPictures(Input),
    /// List bounded NFNT/FONT/FOND resource candidates without selecting a font.
    ListFonts(Input),
    /// Describe the supported subset and versioned machine contract.
    Capabilities,
}
#[derive(Args)]
struct Input {
    /// PICT, resource fork, MacBinary, AppleSingle or AppleDouble file; never modified.
    #[arg(long)]
    input: PathBuf,
    #[arg(long, value_enum, default_value = "auto")]
    container: Container,
    #[command(flatten)]
    limits: ResourceLimits,
}
#[derive(Args, Default)]
struct Selection {
    /// Select an exact signed PICT resource ID, never an ordinal.
    #[arg(long, allow_hyphen_values = true, conflicts_with = "data_fork")]
    resource_id: Option<i16>,
    /// Select the data-fork picture when a container also has PICT resources.
    #[arg(long)]
    data_fork: bool,
}
impl Selection {
    fn value(&self) -> quickdraw::PictureSelector {
        if let Some(id) = self.resource_id {
            quickdraw::PictureSelector::Resource(id)
        } else if self.data_fork {
            quickdraw::PictureSelector::DataFork
        } else {
            quickdraw::PictureSelector::Unique
        }
    }
}
#[derive(Args)]
struct Inspect {
    #[command(flatten)]
    input: Input,
    #[command(flatten)]
    selection: Selection,
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
    Pdf,
}
#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
enum VectorPolicy {
    Reject,
    Raster,
    Preserve,
}
#[derive(Args)]
struct Convert {
    #[command(flatten)]
    input: Input,
    #[command(flatten)]
    selection: Selection,
    #[arg(long)]
    output: PathBuf,
    /// Explicit format, otherwise inferred from .png, .svg or .pdf suffix.
    #[arg(long, value_enum)]
    format: Option<Format>,
    /// Strict rejects known approximate geometry/comments; neither mode is an emulator.
    #[arg(long, value_enum, default_value = "strict")]
    fidelity: Fidelity,
    /// Explicit printer interpretation, separate from the default ignored-comment profile.
    #[arg(long, value_enum, default_value = "ignore")]
    printer_comments: quickdraw::PrinterComments,
    /// SVG/PDF: preserve genuine paths or rasterize the complete composited image.
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
    /// Explicit system highlight color for pictures that do not carry HiliteColor.
    #[arg(long, value_parser = parse_color)]
    highlight: Option<[u8; 3]>,
    /// JSON schema 1 mapping source font ID/size/face to explicit bitmap font sources.
    #[arg(long)]
    font_map: Option<PathBuf>,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FontMap {
    schema: u32,
    fonts: Vec<FontBinding>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FontBinding {
    id: i16,
    size: u16,
    #[serde(default)]
    face: u8,
    path: PathBuf,
    container: Option<Container>,
    resource: Option<quickdraw::FontResourceSelection>,
    family: Option<quickdraw::FontFamilySelection>,
    outline: Option<quickdraw::OutlineSelection>,
}
fn load_fonts(
    path: Option<&Path>,
    limits: &Limits,
    output: &Path,
) -> Result<(quickdraw::FontBook, Vec<PathBuf>)> {
    let mut book = quickdraw::FontBook::default();
    let Some(path) = path else {
        return Ok((book, Vec::new()));
    };
    let mut sources = vec![path.to_path_buf()];
    require_utf8(path)?;
    output_path(path, output, true)?;
    let read = |path: &Path| -> Result<Vec<u8>> {
        require_utf8(path)?;
        let file = File::open(path).map_err(io_error)?;
        if !file.metadata().map_err(io_error)?.is_file() {
            return Err(Error::new(
                ErrorCode::Io,
                "font source must be a regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.take((limits.max_input_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() > limits.max_input_bytes {
            return Err(Error::new(
                ErrorCode::ResourceLimit,
                "font source byte limit exceeded",
            ));
        }
        Ok(bytes)
    };
    let map: FontMap = serde_json::from_slice(&read(path)?).map_err(|e| {
        Error::new(
            ErrorCode::InvalidArguments,
            format!("invalid font map: {e}"),
        )
    })?;
    if map.schema != 1 {
        return Err(Error::new(
            ErrorCode::InvalidArguments,
            "unsupported font map schema",
        ));
    }
    if map.fonts.len() > limits.max_operations {
        return Err(Error::new(
            ErrorCode::ResourceLimit,
            "font binding count limit exceeded",
        ));
    }
    for binding in map.fonts {
        let font_path = path.parent().unwrap_or(Path::new(".")).join(binding.path);
        output_path(&font_path, output, true)?;
        let bytes = read(&font_path)?;
        if let Some(selection) = binding.outline {
            if binding.container.is_some() || binding.resource.is_some() || binding.family.is_some()
            {
                return Err(Error::new(
                    ErrorCode::InvalidArguments,
                    "outline and bitmap container/resource/family selectors are mutually exclusive",
                ));
            }
            let font = quickdraw::OutlineFont::parse(&bytes, selection, limits)?;
            book.insert_outline(binding.id, binding.size, binding.face, font, limits)?;
            sources.push(font_path);
            continue;
        }
        let font = if let Some(container) = binding.container {
            quickdraw::BitmapFont::from_container(
                &bytes,
                container,
                binding.size,
                binding.face,
                &quickdraw::FontSelection {
                    resource: binding.resource,
                    family: binding.family,
                },
                limits,
            )?
        } else {
            if binding.resource.is_some() || binding.family.is_some() {
                return Err(Error::new(
                    ErrorCode::InvalidArguments,
                    "resource/family selection requires an explicit container field (auto is permitted)",
                ));
            }
            quickdraw::BitmapFont::parse(&bytes, limits)?
        };
        book.insert(binding.id, binding.size, binding.face, font, limits)?;
        sources.push(font_path);
    }
    Ok((book, sources))
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
        "formats": ["png", "svg", "pdf"],
        "svg": "explicit_preserved_paths_or_whole_picture_raster",
        "pdf": "pdf_1_4_explicit_preserved_paths_or_whole_picture_raster",
        "vector_policies": ["reject", "preserve", "raster"],
        "vector_profile": "portable_paths_v1",
        "font_map_schema": 1,
        "text_profile": "portable_nfnt_v1",
        "outline_profile": "portable_outline_v1",
        "outline_fonts": {"container":["sfnt","ttc"],"outlines":"static_truetype_glyf","additional_outlines":["static_cff1_name_keyed_type2"],"encoding":"macroman_post_8_5","face_index":"explicit","shaping":"rustybuzz_latin_ltr_und","kerning":"explicit_opt_in","ligatures":"liga_clig_dlig_disabled","classic_fidelity":false,"nonzero_linejustify":false},
        "text_modes": {"base": [0,1,2,3,4,5,6,7,32,33,34,35,36,37,38,39], "mask_flag":64},
        "containers": ["auto", "raw", "file", "resource-fork", "macbinary", "appledouble", "applesingle"],
        "font_resource_types": ["NFNT", "FONT", "FOND"],
        "font_family_scripts": ["roman"],
        "font_family_kerning": "explicit_opt_in_within_text_run",
        "picture_selection": ["unique", "resource_id", "data_fork"],
        "quicktime_still_codecs": ["jpeg", "png ", "raw ", "rle ", "rpza", "smc ", "tiff", "gif "],
        "quicktime_tiff": {"pages":1,"container":"classic_ii_or_mm","storage":"strips_chunky_top_left","samples":"8bit_gray_rgb_unassociated_rgba","compression":["none","packbits","lzw","deflate"],"palette":false,"color_management":false},
        "quicktime_gif": {"images":1,"extent":"full_logical_screen","palette":"explicit_global_or_local","transparency":"explicit_index","interlace":true,"disposal":[0,1],"application_extensions":false,"color_management":false},
        "quicktime_direct_depths": [16,24,32],
        "quicktime_indexed_raw_animation_depths": [1,2,4,8],
        "quicktime_indexed_palette": "embedded_id_0_sequential_flags_0x8000_max_2_power_depth_no_defaults",
        "quicktime_animation_frames": "complete_rows_only_packed_groups_16_16_8_4_monochrome_pair_controls",
        "quicktime_grayscale_depths": [34,36,40],
        "quicktime_grayscale_profile": "explicit_depth_white_zero_linear_ramp_no_color_table",
        "quicktime_description_extensions": ["fiel_progressive_1_0","pasp_square","clap_full_centered","optional_final_zero_u32"],
        "quicktime_rpza_depths": [16,24],
        "quicktime_smc_palette": "depth_8_embedded_id_0_sequential_flags_0x8000_no_default_or_resource_lookup",
        "quicktime_block_frames": "self_contained_only_no_skips_no_uninitialized_caches",
        "quicktime_modes": [0,32,36,256,257,258,260],
        "quicktime_transparency_key": "mode_36_exact_rgb8_equality_to_explicit_opcolor_high_bytes_opaque_sources",
        "quicktime_opcolor_blend": "explicit_opcolor_approximate_whole_picture_vector_fallback",
        "quicktime_affine": "nonsingular_16_16_pixel_center_inverse_with_integral_axis_legacy_sampling",
        "quicktime_alpha_vector": "straight_alpha_only_premultiplied_requires_whole_picture_raster",
        "quicktime_separate_matte": false,
        "printer_comments": {
            "default":"ignore",
            "opt_in":"apple-portable",
            "requires_fidelity":"approximate",
            "text_begin_end_center":[150,151,154],
            "text_rotation":"no_alignment_unmasked_ink_only_exact_fonts_fixed_or_integer_angle",
            "text_fallback":"suppress_copybits_and_ignore_clip_inside_group",
            "width":"comment_182_multiplicative_positive_ratio_axis_aligned_lines_only",
            "synchronization":"empty_190_191_pair_only",
            "postscript_execution":false
        },
        "pict_versions": [1, 2],
        "raster_profile": "portable_rgb_v1",
        "fidelity": {
            "strict": "reject known approximations, not a pixel-identity guarantee",
            "approximate": "allow portable curve/polygon/diagonal coverage, extended NFNT layout/compositing and ignored printer comments with diagnostics",
            "classic_oracle_validated": false
        },
        "supported": vec![
            "rectangles", "rectangular_pen", "8x8_patterns", "rgb_colors",
            "rectangular_and_transition_regions", "ordered_boolean_compositing",
            "monochrome_bitmaps", "indexed_1_2_4_8_bit_pixmaps",
            "rgb555_and_rgb32_pixmaps", "packbits_byte_and_word_runs",
            "pixmap_address_versions_0_4",
            "source_destination_bitmap_rectangles", "bitmap_region_masks",
            "origin_changes", "pixel_patterns", "explicit_highlight_color",
            "arithmetic_transfer_modes_32_to_39", "transparent_pattern_mode_44",
            "explicit_nfnt_single_byte_native_size_faces", "text_start_and_pen_advancement",
            "nfnt_fractional_width_and_image_height_tables",
            "positive_text_ratios", "fractional_space_extra_and_pen_position",
            "text_boolean_arithmetic_transparency_and_mask",
            "svg_and_pdf_uniform_copy_geometry", "whole_picture_svg_pdf_raster",
            "bounded_resource_maps", "macbinary_I_II_III", "appledouble_v2", "applesingle_v2",
            "explicit_font_resources", "fond_exact_bitmap_associations_widths_opt_in_kerning",
            "explicit_picture_selection", "embedded_jpeg_png_axis_aligned_srcCopy",
            "quicktime_raw_rgb555_rgb24_argb32", "quicktime_animation_full_frame_direct_rgb",
            "quicktime_raw_animation_explicit_palette_1_2_4_8",
            "quicktime_raw_animation_explicit_grayscale_2_4_8", "quicktime_neutral_description_extensions",
            "quicktime_image_opcolor_blend_straight_alpha_blend",
            "quicktime_rpza_rgb555_blocks", "quicktime_smc_explicit_palette_blocks",
            "quicktime_affine_rotation_reflection_shear", "quicktime_straight_premultiplied_alpha",
            "vector_bitmap_copy_notCopy_transparent", "vector_tiled_patterns",
            "vector_pixel_run_diagonal_polygon_arc_complex_region_frames"
        ],
        "approximate_only": ["ovals", "rounded_rectangles", "arcs", "polygons", "diagonal_lines", "complex_region_outlines", "printer_comments", "color_bitmap_boolean_modes", "copybits_inactive_dither_hint_no_reduction", "text_scaling_fractional_layout_extended_modes", "quicktime_extended_affine_and_alpha"],
        "copybits_dither_hint": {
            "recorded_modes": [64,65,66,67,68,69,70,71],
            "policy": "approximate only; base source mode with inactive bit 64, never a dithering algorithm",
            "source": "monochrome, RGB555/RGB32, or explicit indexed tables whose every RGB16 component is exactly replicated RGB8",
            "placement": "neither destination dimension may shrink relative to its source rectangle",
            "diagnostic": "copybits_dither_inactive",
            "quicktime_modes_unchanged": true
        },
        "unsupported": [
            "implicit_system_fonts", "classic_outline_mapping", "cff_variable_color_aat_fonts", "multibyte_text", "color_nfnt",
            "script_specific_line_justification", "nonpositive_or_disabled_text_scaling",
            "grayishTextOr_not_recorded_in_pict", "fond_synthetic_styles_and_legacy_negative_fixed",
            "dithering", "default_system_highlight_without_explicit_color",
            "vector_destination_dependent_modes",
            "vector_premultiplied_quicktime_alpha",
            "other_quicktime_codecs", "quicktime_perspective",
            "quicktime_separate_mattes_rendering_affecting_descriptions_other_modes",
            "quicktime_animation_delta_skipped_pixels_undocumented_grayscale_depths",
            "quicktime_block_temporal_dependencies_default_resource_palettes",
            "animated_png_cmyk_jpeg_16bit_still_images",
            "classic_screen_pixel_identity"
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
            "expected convert, inspect, list-pictures, list-fonts, capabilities, --help or --version",
        )
    })? {
        Action::Capabilities => Ok(capabilities()),
        Action::Inspect(args) => {
            let bytes = read_input(&args.input)?;
            let picture = quickdraw::parse_selected(
                &bytes,
                args.input.container,
                args.selection.value(),
                &args.input.limits.value(),
            )?;
            Ok(json!({"protocol":1,"inspection":picture.inspect()}))
        }
        Action::ListPictures(input) => {
            let bytes = read_input(&input)?;
            Ok(
                json!({"protocol":1,"inventory":quickdraw::list_pictures(&bytes,input.container,&input.limits.value())?}),
            )
        }
        Action::ListFonts(input) => {
            let bytes = read_input(&input)?;
            Ok(json!({"protocol":1,"inventory":quickdraw::list_font_resources(&bytes,input.container,&input.limits.value())?}))
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
                        "pdf" => Some(Format::Pdf),
                        _ => None,
                    }
                })
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::InvalidArguments,
                        "select --format png|svg|pdf or use a matching suffix",
                    )
                })?;
            if format != Format::Png && args.vector_policy == VectorPolicy::Reject {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "SVG/PDF requires explicit --vector-policy preserve or raster",
                ));
            }
            if format == Format::Png && args.vector_policy != VectorPolicy::Reject {
                return Err(Error::new(
                    ErrorCode::InvalidArguments,
                    "--vector-policy applies only to SVG/PDF",
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
            let (fonts, font_sources) = load_fonts(
                args.font_map.as_deref(),
                &args.input.limits.value(),
                &args.output,
            )?;
            let options = RenderOptions {
                fidelity: args.fidelity,
                printer_comments: args.printer_comments,
                background: args.background,
                width: args.width,
                height: args.height,
                limits: args.input.limits.value(),
                highlight: args.highlight,
                fonts,
            };
            let picture = quickdraw::parse_selected(
                &bytes,
                args.input.container,
                args.selection.value(),
                &options.limits,
            )?;
            let (encoded, width, height, diagnostics) = if args.vector_policy
                == VectorPolicy::Preserve
            {
                let vector = quickdraw::export_vector(
                    &picture,
                    &options,
                    args.dpi,
                    if format == Format::Pdf {
                        quickdraw::VectorFormat::Pdf
                    } else {
                        quickdraw::VectorFormat::Svg
                    },
                )?;
                (
                    vector.bytes,
                    vector.width,
                    vector.height,
                    vector.diagnostics,
                )
            } else {
                let mut rendered = quickdraw::render(&picture, &options)?;
                let encoded = match format {
                    Format::Png => quickdraw::encode_png(&rendered.image, args.dpi)?,
                    Format::Svg | Format::Pdf => {
                        rendered.diagnostics.push(quickdraw::Diagnostic {
                        code: "raster_fallback",
                        message: "Entire composited picture embedded as an RGB image; no editable vectors."
                            .into(),
                        offset: None,
                    });
                        if format == Format::Pdf {
                            quickdraw::encode_pdf(&rendered.image, args.dpi)?
                        } else {
                            quickdraw::encode_svg(&rendered.image, args.dpi)?
                        }
                    }
                };
                (
                    encoded,
                    rendered.image.width(),
                    rendered.image.height(),
                    rendered.diagnostics,
                )
            };
            // Recheck aliases immediately before committing; rename never follows a target symlink.
            output_path(&args.input.input, &args.output, args.overwrite)?;
            for source in font_sources {
                output_path(&source, &args.output, args.overwrite)?;
            }
            write_output(&target, &encoded, args.overwrite)?;
            Ok(json!({
                "protocol":1, "output":args.output, "format":match format { Format::Png => "png", Format::Svg => "svg", Format::Pdf => "pdf" },
                "width":width, "height":height, "dpi":args.dpi,
                "bytes":encoded.len(), "sha256":format!("{:x}", Sha256::digest(&encoded)),
                "input_sha256":format!("{:x}", Sha256::digest(&bytes)),
                "resource_id":picture.resource_id(),
                "raster_profile":if args.vector_policy == VectorPolicy::Preserve { Value::Null } else { json!("portable_rgb_v1") },
                "vector_profile":if args.vector_policy == VectorPolicy::Preserve { json!("portable_paths_v1") } else { Value::Null },
                "outline_profile":if diagnostics.iter().any(|d|d.code=="explicit_outline_font") { json!("portable_outline_v1") } else { Value::Null },
                "printer_comments":args.printer_comments,
                "diagnostics":diagnostics
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
