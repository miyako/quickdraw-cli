# CLI assets: schema 1

The separate `miyako/quickdraw` component should consume these binaries rather
than maintaining a second interpreter. No release has been published by the
packaging scripts. The native CI workflow uploads build artifacts only.

| Rust target | Platform | Architecture | Binary |
| --- | --- | --- | --- |
| `aarch64-apple-darwin` | `macos` | `arm64` | `bin/quickdraw` |
| `x86_64-apple-darwin` | `macos` | `x86_64` | `bin/quickdraw` |
| `x86_64-pc-windows-msvc` | `windows` | `x86_64` | `bin/quickdraw.exe` |

macOS assets are separate thin binaries, not a universal binary. Windows ARM64
and x86, Linux assets, installer packages and a 4D plugin bundle are not supplied.
These are standalone CLI assets, not the component itself.

## Build and package

On each matching native host, from a **clean committed checkout**:

```sh
cargo test --locked --all-targets
cargo build --locked --release --target aarch64-apple-darwin
python3 scripts/package.py \
  --target aarch64-apple-darwin \
  --binary target/aarch64-apple-darwin/release/quickdraw \
  --output dist/first
```

Use the appropriate target and `.exe` for Windows. Packaging requires Python
3.11+, Rust 1.91.0, the Cargo dependency cache/metadata and Rust's
`COPYRIGHT-library.html`. It fails if redistribution notices cannot be collected.
It verifies native host identity, Mach-O/PE architecture, executable file type
and the binary's JSON `--version` contract. CI builds the binary immediately
before packaging from that same commit; a local caller must do the same, since
matching a version string alone cannot prove source provenance.
Native CI also parses generated SVG/PDF documents with test-only Python
dependencies. These dependencies are not runtime requirements and are not
bundled in the CLI archives.

Files produced (example version):

```text
quickdraw-v0.1.0-aarch64-apple-darwin.zip
quickdraw-v0.1.0-aarch64-apple-darwin.zip.sha256
quickdraw-v0.1.0-aarch64-apple-darwin.manifest.json
```

The ZIP has no outer directory. It includes `bin/quickdraw`, `manifest.json`,
`LICENSE`, `README.md`, CLI/support/asset docs, `Cargo.lock`, and `licenses/`.
The sidecar manifest is byte-identical to the manifest inside the ZIP.
Each manifest includes:

```json
{
  "asset_schema": 1,
  "protocol": 1,
  "name": "quickdraw",
  "version": "0.1.0",
  "target": "aarch64-apple-darwin",
  "platform": "macos",
  "architecture": "arm64",
  "binary": "bin/quickdraw",
  "sha256": "<binary SHA-256>",
  "size": 12345,
  "source_revision": "<checkout commit>",
  "rustc": "<rustc -vV output>",
  "signed": false,
  "notarized": false,
  "files": {
    "bin/quickdraw": {"sha256": "<binary SHA-256>", "size": 12345}
  }
}
```

The shown sizes/hashes are placeholders; the actual `files` inventory covers
every archive file except the manifest itself. The archive digest authenticates
the manifest when obtained from a trusted channel. The `.sha256` sidecar uses
`<digest><two spaces><archive basename><newline>`.

## Verification and installation contract

```sh
python3 scripts/verify_asset.py \
  dist/first/quickdraw-v0.1.0-aarch64-apple-darwin.zip \
  --target aarch64-apple-darwin \
  --sha256 EXPECTED_ARCHIVE_SHA256
```

Verification does not extract or run downloaded code. It checks the archive
digest, size limits, duplicate/unsafe paths, regular-file entries, complete
inventory, every member's size/hash, protocol/platform/architecture, and actual
Mach-O/PE binary headers. This is an integrity check, not a signature check: a
checksum downloaded from an untrusted source with the archive is not proof of
authenticity. The caller must trust the source of the expected archive digest.

A future wrapper should verify before extraction into an application-controlled
directory, preserve executable permissions on macOS, select the host architecture
explicitly, and invoke `--version`/`capabilities` before conversion. Keep the
original PICT separately and retain warnings and content hashes with results.
Do not infer fidelity or feature completeness from an asset being available.
The additive protocol-1 extension exposes `--font-map` schema 1 and explicit
SVG/PDF `preserve`/`raster` policies. Fonts are not bundled or downloaded; a caller
must provide appropriately licensed bitmap font sources and preserve them
alongside the PICT. Probe capabilities rather than assuming every font, codec or
vector operation is supported. Use `source_revision` and archive integrity, not
the development version string alone, to identify an exact milestone build.
Container ingestion adds `list-pictures`, `--resource-id` and `--data-fork`
without changing the envelope. The JPEG decoder is linked into the CLI; the
JPEG encoder, Pillow, pypdf and PDFium are test-only, not external runtime
requirements or files installed beside the binary.
Capabilities also advertise `text_profile: portable_nfnt_v1`; extended text
layout/compositing uses the existing explicit `--fidelity approximate` policy.
Schema 1 now also accepts explicit `container`/`resource`/`family` selections,
with `list-fonts` inventory and AppleSingle v2. Old raw-font maps are unchanged.
FOND family selection requires a declared Roman script; pair kerning is an
explicit approximate-policy opt-in, not inferred from font availability.
A wrapper must
surface unsupported-font/encoding errors rather than retry with guessed fonts.
Further additive capabilities advertise direct-color raw/Animation stills,
nonsingular affine placement and recorded alpha modes. Extended affine/alpha
requires approximate fidelity; premultiplied-alpha vector output requires the
explicit complete-raster policy. Separate matte payloads remain unsupported.
Diagonal/polygon/arc/complex-region frames now preserve shared pixel-run paths.
Protocol 1, archive layout and existing raw-font/opaque-image behavior are unchanged.
RPZA and explicitly paletted SMC still decoding are also additive capabilities.
Their temporal skips, missing caches and unavailable palettes remain errors;
consumers must not retry by supplying guessed prior pixels or system palettes.
Indexed raw/Animation depths 1/2/4/8 are additive, requiring the same explicit
embedded palette contract. Capabilities retain the direct-depth field and add
`quicktime_indexed_raw_animation_depths`, `quicktime_indexed_palette` and
`quicktime_animation_frames`; no protocol, invocation or archive-schema break.
Further additions expose explicit grayscale depths 34/36/40, validated neutral
description suffixes and OpColor image modes 32/260. Inspection optionally retains
suffix bytes/offset; consumers must preserve the new diagnostic and require
whole-picture vector fallback for image blending. Existing outline profiles,
versioned envelopes and asset schema remain unchanged.
Schema-1 font maps now optionally select `outline:{face_index,encoding:"macroman",
kerning:false}` for exact static TrueType or supported name-keyed CFF1 sfnt/TTC
input. The original `outline_fonts.outlines` capability stays unchanged;
`additional_outlines` advertises the added CFF1 profile without changing the
existing field's type or value. `nonzero_linejustify:false` makes its evidence
boundary explicit. `portable_outline_v1`
is separately reported and requires approximate fidelity; bitmap bindings and
protocol 1 remain compatible. Packages include the locked portable shaper/parser
and their redistribution notices, not platform font dependencies or user fonts.

Embedded `tiff` and `gif ` are additive `quicktime_still_codecs` entries.
`quicktime_tiff` and `quicktime_gif` describe the exact single-page/image,
layout, palette/alpha and color-management boundaries. No frame-selection
argument or silent first-frame behavior is introduced. The portable TIFF/GIF,
LZW and Deflate libraries and their license notices travel inside the same
native assets; Pillow/libtiff remain fixture-generation/test dependencies only.
Font profiles, protocol 1 and asset schema 1 are unchanged.

Archives use sorted members, a fixed 1980 timestamp, explicit Unix permissions,
canonical JSON and stored (uncompressed) members. Identical binary, source/docs,
dependency notices, and toolchain metadata produce **byte-identical archives**;
CI packages twice and compares them. This does not claim bit-identical native
compiler output across different hosts, SDKs, linkers or OS images.

The license bundle is intentionally over-inclusive target-filtered Cargo
resolution (including build tools), plus Rust runtime notices. It is not an SBOM
or a claim of exact linked-component coverage.

## Distribution limitations

Artifacts have no publisher signature and are not notarized; `signed: false`
does not exclude a linker's automatic ad-hoc Mach-O signature. Platform quarantine, Gatekeeper and
Windows trust policies may require additional release work. The CLI workflow has
read-only repository permissions, no signing secrets and no release publishing
step. CI downloads are retained for 30 days and are not a durable public release
channel. Publishing versioned release assets and integrating the wrapper require
a later readiness decision.
