# quickdraw 0.1.0

Standalone cross-platform PICT conversion library and CLI, with an explicitly
documented portable feature set. Includes the illustrated feature guide.

## Downloads

| Archive | Executable |
| --- | --- |
| `quickdraw-v0.1.0-universal-apple-darwin.zip` | `bin/quickdraw`: one universal arm64 + x86_64 Mach-O, Developer ID signed after combining slices, hardened runtime, accepted Apple notarization |
| `quickdraw-v0.1.0-x86_64-pc-windows-msvc.zip` | `bin/quickdraw.exe`: native Windows x64; no Authenticode signature claimed |

Each ZIP has a checksum and schema-1 manifest sidecar. Keep the full license/docs
payload. Protocol 1 and font-map schema 1 remain unchanged. The universal macOS
manifest uses `target: universal-apple-darwin`, `architecture: universal`,
`architectures: [arm64, x86_64]`. It replaces the two thin macOS CI downloads
for release consumers; do not combine or re-sign it.

The manifest records the exact source revision, binary/file hashes, Developer
team and accepted notarization submission. Standalone executables and ZIPs cannot
be stapled like application bundles/DMGs; notarization relies on Apple's online
ticket service. Packaging is reproducible for identical **already signed** bytes,
not across separate timestamped signing/notarization runs.

## Scope and limitations

Supports PICT v1/v2; bounded legacy containers; core drawing/state/regions,
patterns, PixMaps and documented transfer profiles; exact supplied NFNT/FONT/FOND
bindings; explicit portable MacRoman TrueType/CFF1 outlines; supported QuickTime
stills; PNG and genuine SVG/PDF geometry or explicit full-composite raster.
See `docs/features.md`, `docs/support.md` and `docs/cli.md` in the archives.

This is **not a fully capable QuickDraw emulator or a general archival migration
tool**. Approximate geometry/font/printer behavior is opt-in. Nonzero lineJustify,
separate PICT mattes, actual device dithering/palette search, unsupported scripts,
temporal image dependencies and other excluded variants fail explicitly.
No implicit fonts, palettes or prior frames are substituted. Source files are
protected; retain diagnostics and originals. Classic-Mac pixel fidelity is not
established. Destination-dependent vector operations require the explicit
whole-picture raster policy.

All three native architectures pass the release workflow's tests before
publication. Published assets are downloaded, their complete inventories and
native identities verified, and the final universal signature rechecked before
the draft becomes public. This release is the CLI, not a 4D component bundle.
