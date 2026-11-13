# Supported subset and fidelity evidence

## Scope

The decoder is original Rust code informed by published PICT structures. It is
not a port of a complete QuickDraw implementation. The core keeps parsing,
ordered state/operations, software compositing and output encoding separate.
Strict mode means **no known unsupported/approximate operation accepted**; it
does not mean pixel identity with a classic Mac. Approximate mode is opt-in and
emits an operation-offset warning for every accepted approximation.

| Feature / opcodes (hex) | Status |
| --- | --- |
| v1 byte opcodes; v2 word alignment, original/extended HeaderOp | Supported; nonzero padding rejected; required v2 header |
| Extended header resolution and source rectangle | Resolution retained; source rectangle must equal picFrame; other mappings fail |
| Raw PICT and 512-byte-header PICT files | Supported, explicit or unambiguous auto-detection |
| Resource fork / MacBinary I-II-III / AppleDouble v2 | Bounded extraction, signed resource IDs, inventory and explicit data-fork/resource selection; no filesystem extraction or companion-file lookup |
| Rectangular and transition-encoded regions (`01`, `80`-`84`) | Clipping, paint, fill, erase, invert; strict rectangular frames; complex frames approximate |
| Background/pen/fill 8x8 patterns (`02`, `09`, `0A`) | Supported, distinct state |
| Pen size/mode (`07`, `08`) | Nonnegative rectangular pens; Boolean, arithmetic and highlight modes below |
| Foreground/background (`0E`, `0F`, `1A`, `1B`) | Eight classic color constants and 16-bit RGB components reduced to high 8 bits |
| Lines (`20`-`23`) | Integer pen, signed short deltas; axis-aligned strict; diagonals approximate |
| Rectangles (`30`-`34`, `38`-`3C`) | Frame, paint, erase, invert, fill; shared last-rectangle state |
| Ovals (`50`-`54`, `58`-`5C`) | Approximate integer pixel-center ellipse coverage; shared last rectangle, even across shape families |
| Polygons (`70`-`74`) | Approximate even-odd pixel-center fill; integer stroked closed outlines |
| Same polygon / same region (`78`-`7C`, `88`-`8C`) | Explicitly unsupported (also marked unimplemented in Apple's opcode appendix) |
| Mono BitsRect/BitsRgn/PackBitsRect/PackBitsRgn (`90`, `91`, `98`, `99`) | Supported; source/destination rectangles, region masks, foreground/background mapping |
| Indexed PixMaps in v2 | 1/2/4/8-bit; color-table index or sequential device-table entries; absent/duplicate indices rejected |
| DirectBitsRect/DirectBitsRgn (`9A`, `9B`) | RGB555 and RGB32; RGB only, high pad byte is not alpha |
| Packing | Byte PackBits; word PackBits for RGB555; RGB32 unpacked, pad-dropped, planar component packing; 3/4 component layouts |
| CopyBits modes 0-7 | Mono strict; non-copy color PixMaps require approximate mode |
| v2 reserved opcodes with documented sizes | Retained/skipped with `reserved_opcode` diagnostics; unknown/v2-only opcodes in v1 fail |
| Text state and font-name metadata | Exact font ID/size/face selection; font names retained without system lookup; unsupported layout settings fail at text use |
| Length-prefixed text settings (`2D`, `2E`) | Known 8-byte lineJustify / 4-byte glyphState bodies; short bodies malformed, unknown extensions retained but unsupported, not guessed |
| Text (`28`-`2B`) | Explicit native-size one-bit NFNT/FONT bindings; srcOr/srcBic; integer metrics and spacing; deltas from previous text start; advances graphics pen |
| Printer comments (`A0`, `A1`) | Raw bytes and kind retained; strict fails; approximate warns that comment semantics are ignored |
| Origin (`0C`) | Horizontal/vertical delta; local clip/pen/last-shape state preserved; prior pixels do not move |
| Oval size / rounded rectangles (`0B`, `40` family) | Approximate elliptical corners, clamped diameters, inset framing |
| Arcs (`60` family) | Approximate elliptical sectors; zero at 12 o'clock, positive clockwise; signed sweeps; frame excludes radii |
| Pixel patterns (`12`-`14`) | Bit, solid RGB and indexed color tiles; bounded shared PixMap decoding; color patterns copy directly without pen-mode/fg/bg mapping |
| OpColor / arithmetic modes (`1F`, modes 32-39) | Component blend, pin/wrap add/subtract, transparency, max/min; blend and pin modes require explicit OpColor |
| Highlight (`1C`-`1E`, modes 50/52/58/60) | Exchanges background/highlight colors, leaves other destination colors intact; one-shot HiliteMode; explicit HiliteColor or CLI highlight required |
| Transparent pattern mode 44 | Whole-pixel background keying |
| Dithering / other extended modes | Unsupported; never silently mapped to copy |
| QuickTime compressed (`8200`) | JPEG/PNG still decoding; restricted positive scale/translation, integral edges, source crop/mask, clip/origin and direct RGB srcCopy |
| QuickTime uncompressed (`8201`) / other codecs | Explicitly unsupported; no silent substitute decoder |
| Unknown opcodes / unknown length layouts | Fail with offset and opcode; no recovery by guessing |
| AppleSingle / packed MacBinary / native resource-fork filesystem access | Unsupported; supply a supported file container |
| PNG | Opaque RGB output |
| SVG / PDF | Explicit whole-picture raster or supported genuine-path policy; PDF 1.4 single page |
| Native vector subset | Source-only tiled patterns and embedded bitmap placements, shapes/regions, axis-aligned lines and NFNT runs; destination-dependent modes and diagonal/polygon/arc/complex-region frames require explicit whole-picture raster |
| Remaining embedded-image gaps | Mattes, nonopaque alpha, noncopy QuickTime modes, rotation/reflection/perspective, fractional edges, extended descriptions, animation, CMYK/16-bit images |
| Outline/scaled/fractional/multibyte text, optional NFNT tables | Unsupported; no silent substitution |
| Classic screen emulation | Not implemented |

## Raster profile `portable_rgb_v1`

The framebuffer has picFrame dimensions. Rectangles use half-open pixel ranges:
left/top inclusive, right/bottom exclusive. Rectangular outlines lie inside the
bounds; pen width and height control the inset independently. Zero pen dimension
suppresses framing and lines but not painting/filling. Clip state is replaced by
each Clip opcode and intersected with the current local-coordinate picture frame
at drawing time. Origin deltas move that frame while retaining local clip, pen
and remembered shape coordinates; existing framebuffer contents stay fixed.
Regions XOR horizontal transition spans into active scanlines, preserving holes,
disconnected areas and negative coordinates. Malformed transition ordering,
out-of-bounds coordinates, odd counts and unclosed regions fail.

Patterns are anchored to signed **picture coordinate** multiples of eight, not
output dimensions; unknown historical screen/global pattern phase is not
reconstructed. The initial framebuffer color does not alter QuickDraw foreground
and background state (initially black and white). Paint/frame use the pen pattern
and mode; fill uses FillPat with copy; erase uses BkPat with copy; invert complements
the existing framebuffer, independent of pen settings.

Monochrome Boolean modes use logical ink (one = black), mapped to RGB (zero = black).
For source ink `s` and destination ink `d`, modes 0-7 are
copy `s`, or `s|d`, xor `s^d`, bic `(~s)&d`, not-copy `~s`, not-or `(~s)|d`,
not-xor `(~s)^d`, not-bic `s&d`. Pattern modes 8-15 use the same truth tables.
RGB copy mixes foreground/background by the source channel intensity. RGB XOR
inverts destination for black source pixels (white for not-XOR), leaving colored
source pixels alone; it does not claim a historical indexed-device palette rule.
Non-copy color PixMaps are explicitly approximate.
Color pixel patterns bypass foreground/background and mode, per Apple's
Color QuickDraw description; the pattern sample is copied directly.

Arithmetic works on raw 8-bit RGB source/destination components, without
foreground/background tinting. Blend uses all 16 OpColor bits as the source
weight. Pin limits use OpColor's high byte; wrapping add/subtract are modulo 256.
Transparency compares the whole source pixel with the background color, not
individual components. OpColor's initial value and the system highlight color
are not invented. HiliteMode affects the next drawing operation, even an empty
one, and is reset afterward; it is not consumed by intervening state changes.

Lines use endpoint-inclusive integer Bresenham coverage, with the pen hanging
below/right. All pen stamps in one line/polygon operation are unioned **before**
compositing, so overlapping thick stamps do not cancel one another under XOR.
Curves use integer pixel-center tests; polygon fill uses even-odd crossings;
polygon outlines are closed by the renderer. Complex region outlines use
rectangular-kernel erosion. These are deliberate approximations, not recovered
Apple edge/tie-breaking algorithms. Separate operations always composite in
source order into the existing destination.

Bitmaps use nearest-neighbor destination mapping and respect clip and optional
mask regions. Padding bytes/bits are never rendered. Direct 16-bit channels expand
5 bits to 8 by bit replication; direct 32-bit high components are padding, not
opacity. Row lengths are bounded before decoding, repeat controls cannot cross a
row boundary, and decoded rows must have exactly the expected byte count.
Empty/inverted source bitmap rectangles fail; empty destination rectangles draw
nothing. Negative-size geometric rectangles have empty coverage.

## Validation and provenance

All fixtures in `tests/common`, `tests/core.rs`, `tests/bitmaps.rs` and
`tests/cli.rs`, `tests/semantics.rs`, `tests/fonts.rs`, `tests/vectors.rs` and
`tests/containers.rs`, `tests/quicktime.rs` and `scripts/test_documents.py` are
authored synthetically in this repository. No reference
repository pictures or expected image files are copied. Tests cover both PICT
versions, header containers/alignment, lengths and truncation, mutation sweeps,
limits, PackBits byte/word boundaries, indexed/direct pixels, clipped patterns,
region holes, overlapping XOR, source/destination scaling, deterministic PNG,
and process-level JSON/error/output safety.
Additional tests cover origin/clip/pen interaction, rounded corners, arc
direction/radial exclusion, pixel tiles, arithmetic and highlight behavior,
NFNT offsets/metrics/truncation, explicit mapping and source protection,
text-start versus advanced pen coordinates, genuine paths and vector rejection.
pypdf independently checks generated PDF xrefs, page dimensions, content streams
and complete-composite image bytes; the XML parser checks SVG structure/clipping.
These are document interoperability checks, not a classic rendering oracle.
Container tests construct original maps/headers, exercise every truncation and
selected offset/overlap/duplicate/CRC failures, and verify signed IDs and absolute
opcode offsets. Embedded PNG fixtures are authored pixel arrays encoded by the
existing PNG crate/Pillow; JPEG fixtures are generated from authored RGB/CMYK
arrays by the independent `jpeg-encoder` test dependency. Actual decoding,
placement and observable failures are tested; JPEG pixel assertions allow lossy
rounding. No compressed-image fixture is copied from another repository.
PDFium independently rasterizes source-only bitmap/pattern PDF output and is
compared with this project's raster interpreter for selected synthetic cases,
including masks, negative picture origins and transparent pattern holes.
This is independent output-backend evidence, not independent QuickDraw fidelity.

The tests' truth tables and selected expected pixels establish the declared
implementation profile, **not independent rendering fidelity**. The mutation
sweep is a bounded deterministic regression test, not exhaustive fuzzing.
The archive tests use deliberately synthetic PE/Mach-O headers only to test
identity checks; real native artifacts must separately pass the packaging gate.

No classic-Mac emulator, historical fonts, printer environment, real-world user
corpus or independent pixel oracle was supplied. Accordingly no validation of
pixel-perfect classic output, text layout or general printer output is claimed.
ImageMagick and QuickDrawViewer are not used as authoritative fidelity oracles.
Native CI results must be read from actual workflow runs; merely adding Windows
jobs is not evidence that Windows tests passed.

## References (format research, not copied implementation)

* [Apple, picture version/header opcodes](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-459.html)
* [Apple, opcode data types](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-460.html)
* [Apple, PICT opcode and PixMap packing tables](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-461.html)
* [Apple, QuickDraw image/pen/shape semantics](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-12.html)
* [Apple, printer comments](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-465.html)
* [Apple, resource file format](https://dev.os9.ca/techpubs/mac/MoreToolbox/MoreToolbox-99.html)
* [MacBinary II specification](https://files.stairways.com/other/macbinaryii-standard-info.txt)
* [RFC 1740, AppleDouble layout](https://www.rfc-editor.org/rfc/rfc1740.html)
* QuickDrawViewer `QuickTime.swift` (read-only format comparison): compressed
  picture fields and source-region transition layout; no implementation copied.
  Matrix fields follow QuickTime 16.16 affine and 2.30 projective conventions.
* [Apple, SetOrigin semantics](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-40.html)
* [Apple, FrameRoundRect](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-103.html)
* [Apple, FrameArc](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-115.html)
* [Apple, Color QuickDraw modes and pixel patterns](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-199.html)
* [Apple, OpColor](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-230.html)
* [Apple, NFNT structure/metrics](https://dev.os9.ca/techpubs/mac/Text/Text-250.html),
  [flags](https://dev.os9.ca/techpubs/mac/Text/Text-251.html),
  [extended offsets](https://dev.os9.ca/techpubs/mac/Text/Text-252.html)
* [Apple, PICT font identities/text placement](https://dev.os9.ca/techpubs/mac/Text/Text-142.html),
  [text transfer modes](https://dev.os9.ca/techpubs/mac/Text/Text-137.html)
* FontForge `fontforge/macbinary.c` NFNT parser (read-only comparison):
  corroborates the width table offset relative to byte 16; no implementation copied.
* [QuickDrawViewer](https://github.com/wiesmann/QuickDrawViewer), Apache-2.0:
  region transition interpretation and scope comparison. Its Apple-framework
  printer-style renderer is not this project's cross-platform backend or oracle.

## Readiness

This is a usable **subset foundation**, not a fully capable conversion backbone.
The versioned CLI and assets allow future wrapper integration work, but broad
wrapper readiness remains limited by outline/scaled/multibyte font handling,
remaining QuickTime codecs/transforms, modes/containers, vector coverage and
independent corpus/fidelity validation. Explicit bitmap fonts and supported
vector paths are usable, not a claim of complete font or printer support. A wrapper must not
hide those failures or discard success warnings. No wrapper component, release
publication, code signing or notarization is included in this repository.
