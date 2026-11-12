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
| Rectangular and transition-encoded regions (`01`, `80`-`84`) | Clipping, paint, fill, erase, invert; strict rectangular frames; complex frames approximate |
| Background/pen/fill 8x8 patterns (`02`, `09`, `0A`) | Supported, distinct state |
| Pen size/mode (`07`, `08`) | Nonnegative rectangular pens; pattern modes 8-15 only |
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
| Text state, font-name and glyph metadata | Retained opaquely; not used to draw or select fonts |
| Text (`28`-`2B`) | Parsed for inspection, rendering always fails; no font substitution |
| Printer comments (`A0`, `A1`) | Raw bytes and kind retained; strict fails; approximate warns that comment semantics are ignored |
| Origin (`0C`), highlight (`1C`) | Parsed, rendering fails |
| Rounded rectangles and arcs (`40` family, `60` family) | Parsed, rendering fails |
| Pixel patterns (`12`-`14`) | Layout not implemented; parsing fails explicitly |
| Arithmetic modes, dithering, mask/transparent modes | Unsupported; never silently mapped to copy |
| QuickTime (`8200`, `8201`) | Length/payload retained; codec rendering fails |
| Unknown opcodes / unknown length layouts | Fail with offset and opcode; no recovery by guessing |
| Resource forks, MacBinary, AppleDouble | Unsupported containers; extract a single PICT resource first |
| PNG | Opaque RGB output |
| SVG | Opt-in whole-picture embedded PNG, never claimed editable vectors |
| PDF / native vector / screen emulation | Not implemented |

## Raster profile `portable_rgb_v1`

The framebuffer has picFrame dimensions. Rectangles use half-open pixel ranges:
left/top inclusive, right/bottom exclusive. Rectangular outlines lie inside the
bounds; pen width and height control the inset independently. Zero pen dimension
suppresses framing and lines but not painting/filling. Clip state is replaced by
each Clip opcode and intersected with the fixed picture frame at drawing time.
Regions XOR horizontal transition spans into active scanlines, preserving holes,
disconnected areas and negative coordinates. Malformed transition ordering,
out-of-bounds coordinates, odd counts and unclosed regions fail.

Patterns are anchored to signed **picture coordinate** multiples of eight, not
output dimensions; unknown historical screen/global pattern phase is not
reconstructed. The initial framebuffer color does not alter QuickDraw foreground
and background state (initially black and white). Paint/frame use the pen pattern
and mode; fill uses FillPat with copy; erase uses BkPat with copy; invert complements
the existing framebuffer, independent of pen settings.

Boolean modes use logical ink (one = black), mapped to RGB (zero = black), with
per-channel arithmetic. For source ink `s` and destination ink `d`, modes 0-7 are
copy `s`, or `s|d`, xor `s^d`, bic `(~s)&d`, not-copy `~s`, not-or `(~s)|d`,
not-xor `(~s)^d`, not-bic `s&d`. Pattern modes 8-15 use the same truth tables.
This is not an emulation of every Color QuickDraw indexed device, palette or
color-matching rule. Non-copy color PixMaps are explicitly approximate; colored
patterns use the declared RGB profile, not a historical device oracle.

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
`tests/cli.rs` are authored synthetically in this repository. No reference
repository pictures or expected image files are copied. Tests cover both PICT
versions, header containers/alignment, lengths and truncation, mutation sweeps,
limits, PackBits byte/word boundaries, indexed/direct pixels, clipped patterns,
region holes, overlapping XOR, source/destination scaling, deterministic PNG,
and process-level JSON/error/output safety.

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
* [QuickDrawViewer](https://github.com/wiesmann/QuickDrawViewer), Apache-2.0:
  region transition interpretation and scope comparison. Its Apple-framework
  printer-style renderer is not this project's cross-platform backend or oracle.

## Readiness

This is a usable **subset foundation**, not a fully capable conversion backbone.
The versioned CLI and assets allow future wrapper integration work, but broad
wrapper readiness is blocked by text/fonts, embedded QuickTime codecs, missing
state/shapes/modes and independent corpus/fidelity validation. A wrapper must not
hide those failures or discard success warnings. No wrapper component, release
publication, code signing or notarization is included in this repository.
