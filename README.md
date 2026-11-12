# quickdraw-cli

A standalone Rust QuickDraw PICT parsing library and conversion CLI for macOS and
Windows. Converts a **documented subset** to opaque PNG, or to explicitly
raster-backed SVG. No Apple graphics frameworks, external converters, fonts, or
network services are used during conversion.

**Not a fully capable QuickDraw emulator or a general archival migration tool.**
Text, fonts, QuickTime images, origin changes, arcs, rounded rectangles, pixel
patterns, and several transfer modes remain unsupported. Unsupported drawing
fails instead of disappearing. Classic-Mac pixel fidelity has not been established.
The separate `miyako/quickdraw` 4D component is not implemented here.

```sh
cargo build --locked --release
target/release/quickdraw --version
target/release/quickdraw capabilities
target/release/quickdraw inspect --input drawing.pict
target/release/quickdraw convert --input drawing.pict --output drawing.png
target/release/quickdraw convert --input drawing.pict --output drawing.svg \
  --fidelity approximate --vector-policy raster
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
The renderer interprets them into a single destination framebuffer. Encoders
consume the final image; no backend can accidentally drop a destination-dependent
operation or substitute a font. Library API stability follows the crate's
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
```

Tests construct original synthetic PICT data, verify specific pixels and failure
paths, and exercise the CLI as a subprocess. Their expected output is not an
independent classic-Mac oracle. Native macOS ARM64, macOS x86_64 and Windows x86_64
CI jobs run tests before producing unsigned archives. CI artifacts are not
published releases.

MIT licensed. The implementation is original; format research references are
listed in [support.md](docs/support.md). Distributable archives include dependency
and Rust runtime license notices.
