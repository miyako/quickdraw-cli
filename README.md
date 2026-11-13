# quickdraw-cli

A standalone Rust QuickDraw PICT parsing library and conversion CLI for macOS and
Windows. Converts a **documented subset** to opaque PNG, or to SVG/PDF with
explicit preserved-path or whole-picture raster policies. Supports explicitly
supplied bitmap fonts. No Apple graphics frameworks, external converters, system
font search, or network services are used during conversion.

**Not a fully capable QuickDraw emulator or a general archival migration tool.**
Origin changes, rounded rectangles, arcs, pixel patterns, arithmetic/highlight
modes and one-bit NFNT text are implemented with explicit limitations. Text
supports optional width/height tables, ratio scaling, fractional spacing and
documented source transfer modes under an explicit portable policy.
Classic PixMap depths/packing include address versions 0/4. CopyBits' dither
hint is accepted only with approximate policy and no size/precision reduction;
actual dithering and implicit device palettes remain unsupported.
Resource forks, MacBinary, AppleDouble and AppleSingle support explicit picture
and font-resource selection. Explicit Roman FOND bindings provide exact bitmap
associations, family fractional widths and opt-in within-run pair kerning.
Embedded JPEG/PNG, single-page 8-bit TIFF, single-image GIF, direct-color and
explicitly paletted 1/2/4/8-bit raw/Animation, Apple Video (RPZA), and Apple
Graphics (SMC with an explicit embedded palette) still images support bounded
decoding without temporal-frame guesses. Nonsingular affine placement and recorded straight/premultiplied alpha
use an explicit portable policy. Separate matte payloads remain unsupported.
TIFF/GIF use linked portable decoders with explicit layout, transparency and
resource limits; additional pages/frames and unsupported color profiles fail.
Raw/Animation also support declared white-zero grayscale depths. Neutral image
description hints are validated and retained; OpColor blending requires an
explicit weight and whole-picture vector fallback.
QuickTime OpColor transparency keying preserves image layers. An opt-in
`--printer-comments apple-portable` profile supports bounded text rotation and
fractional axis-aligned line widths, separately from default comment handling;
it never executes PostScript or combines text with its fallback bitmap.
Diagonal pens and polygon/arc/complex-region frames can export shared pixel-run paths.
Explicit portable MacRoman text can use caller-supplied static TrueType or supported
name-keyed CFF1 sfnt/TTC
faces with deterministic shaping and genuine SVG/PDF glyph paths. Classic
outline mapping, multibyte text, other QuickTime codecs/projective transforms and several
vector modes remain unsupported. Unsupported drawing fails instead of disappearing.
Classic-Mac pixel fidelity has not been established.
The separate `miyako/quickdraw` 4D component is not implemented here.

```sh
cargo build --locked --release
target/release/quickdraw --version
target/release/quickdraw capabilities
target/release/quickdraw inspect --input drawing.pict
target/release/quickdraw list-pictures --input archive.bin
target/release/quickdraw list-fonts --input fonts.as
target/release/quickdraw convert --input archive.bin --resource-id 128 --output selected.png
target/release/quickdraw convert --input drawing.pict --output drawing.png
target/release/quickdraw convert --input drawing.pict --output drawing.svg \
  --fidelity approximate --vector-policy raster
target/release/quickdraw convert --input geometry.pict --output geometry.pdf \
  --fidelity approximate --vector-policy preserve
target/release/quickdraw convert --input text.pict --output text.png --font-map fonts.json
```

On Windows, use `target/release/quickdraw.exe`. All paths are explicit; output files
are never overwritten unless requested, and input aliases are always protected.
Every conversion reports fidelity diagnostics, even on success.

| Document | Purpose |
| --- | --- |
| [CLI contract](docs/cli.md) | Arguments, JSON protocol 1, diagnostics, exit codes, limits |
| [Support and validation](docs/support.md) | Opcode/semantics matrix, approximations, evidence and gaps |
| [Binary assets](docs/assets.md) | Native build matrix, archive layout, integrity and wrapper handoff |

## Reusable library

The package is `quickdraw-cli`; the library crate is `quickdraw`.

```rust
use quickdraw::{Container, Limits, RenderOptions, parse, render, encode_png};

fn example(bytes: &[u8]) -> quickdraw::Result<()> {
    let picture = parse(bytes, Container::Auto, &Limits::default())?;
    let report = picture.inspect(); // Parsing is not a renderability guarantee.
    let rendered = render(&picture, &RenderOptions::default())?;
    let png_bytes = encode_png(&rendered.image, 72.0)?;
    // Retain rendered.diagnostics with the converted file.
    // Use report and png_bytes in the calling application.
    Ok(())
}
```

The parser retains ordered operations, source offsets, header bytes and comments.
The raster renderer interprets them into a single destination framebuffer.
`export_vector` emits genuine paths for supported operations, sharing state and
font layout with raster rendering; it rejects unsupported vector operations
instead of silently substituting pixels. `encode_pdf` and `encode_svg` embed a
complete rendered image when the caller explicitly chooses that policy.
`BitmapFont::parse` and `FontBook::insert` supply exact font bindings.
`list_font_resources`, `FontFamily::parse` and `BitmapFont::from_container`
support bounded font containers and explicit resource/family selection.
Library API stability follows the crate's
pre-1.0 version; the separately versioned CLI envelope is intended for
`4D.SystemWorker`.

## Development

Rust 1.91.0 is pinned in `rust-toolchain.toml`; dependencies are locked.
Packaging additionally requires Python 3.11 or newer.

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
python3 scripts/test_assets.py
python3 -m venv .venv-documents
.venv-documents/bin/python -m pip install -r scripts/requirements-test.txt
.venv-documents/bin/python scripts/test_documents.py
```

On Windows, the virtual environment interpreter is
`.venv-documents/Scripts/python.exe`. The document tests require the debug CLI
built by `cargo test`; `QUICKDRAW_BINARY` can select a different native binary.
Remove the local test environment when finished; it is not part of an asset.

Tests construct original synthetic PICT data, verify specific pixels and failure
paths, and exercise the CLI as a subprocess. Their expected output is not an
independent classic-Mac oracle. The document tests independently parse PDF with
pypdf and SVG with Python's XML parser. PDFium also independently renders selected
bitmap/pattern output for comparison with the raster backend. These checks do
not establish historical QuickDraw fidelity.
Native macOS ARM64, macOS x86_64 and Windows x86_64
CI jobs run tests before producing unsigned archives. CI artifacts are not
published releases.

MIT licensed. The implementation is original; format research references are
listed in [support.md](docs/support.md). Distributable archives include dependency
and Rust runtime license notices.
