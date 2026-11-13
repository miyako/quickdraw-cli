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
| Resource fork / MacBinary I-II-III / AppleDouble and AppleSingle v2 | Bounded extraction, signed resource IDs, inventory and explicit data-fork/resource selection; no filesystem extraction or companion-file lookup |
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
| Text (`28`-`2B`) | Explicit one-bit NFNT/FONT bindings; source modes 0-7, arithmetic/transparent 32-39 and mask 64; unioned run coverage, deltas from previous text start, advanced graphics pen |
| Text ratios/fractions (`06`, `10`, `15`, `16`, `2E`) | Positive independent ratios, signed Fixed SpExtra, PnLocHFrac, integer PICT ChExtra, explicit fractional-width selection; nonnative layout requires approximate policy |
| NFNT optional tables | Bounded 8.8 glyph advances and top-row/image-height entries; no silently omitted ink |
| Font-resource selection / FOND | Explicit NFNT/FONT IDs or exact intrinsic FOND associations; mandatory Roman family assertion; optional fractional family widths after NFNT precedence; explicit approximate within-run kerning policy |
| Printer comments (`A0`, `A1`) | Raw bytes and kind retained; strict fails; approximate warns that comment semantics are ignored |
| Origin (`0C`) | Horizontal/vertical delta; local clip/pen/last-shape state preserved; prior pixels do not move |
| Oval size / rounded rectangles (`0B`, `40` family) | Approximate elliptical corners, clamped diameters, inset framing |
| Arcs (`60` family) | Approximate elliptical sectors; zero at 12 o'clock, positive clockwise; signed sweeps; frame excludes radii |
| Pixel patterns (`12`-`14`) | Bit, solid RGB and indexed color tiles; bounded shared PixMap decoding; color patterns copy directly without pen-mode/fg/bg mapping |
| OpColor / arithmetic modes (`1F`, modes 32-39) | Component blend, pin/wrap add/subtract, transparency, max/min; blend and pin modes require explicit OpColor |
| Highlight (`1C`-`1E`, modes 50/52/58/60) | Exchanges background/highlight colors, leaves other destination colors intact; one-shot HiliteMode; explicit HiliteColor or CLI highlight required |
| Transparent pattern mode 44 | Whole-pixel background keying |
| Dithering / other extended modes | Unsupported; never silently mapped to copy |
| QuickTime compressed (`8200`) | JPEG/PNG, direct and explicitly paletted 1/2/4/8-bit raw/Animation, RGB555 RPZA and explicitly paletted SMC self-contained stills; exact explicit boundaries in CLI contract |
| QuickTime affine/alpha | Nonsingular affine rotation/reflection/shear/fractional placement; straight and premultiplied white/black alpha, explicit approximate policy; source mask, clip and origin preserved |
| QuickTime uncompressed (`8201`) / other codecs | Explicitly unsupported; no silent substitute decoder |
| Unknown opcodes / unknown length layouts | Fail with offset and opcode; no recovery by guessing |
| AppleSingle/AppleDouble v1 / packed MacBinary / native resource-fork filesystem access | Unsupported; supply a supported file container |
| PNG | Opaque RGB output |
| SVG / PDF | Explicit whole-picture raster or supported genuine-path policy; PDF 1.4 single page |
| Native vector subset | Tiled patterns, copied/straight-alpha image layers, shapes/regions and NFNT runs; diagonal/polygon/arc/complex-region frames use shared pixel-run coverage |
| Remaining embedded-image gaps | Separate matte payloads, projective matrices, other transfer modes, extensions, temporal/skipped-pixel Animation, implicit palettes/grayscale depth codes, planar raw, CMYK/16-bit-channel images; preserved premultiplied alpha needs whole-picture raster |
| Portable outline text | Explicit static TrueType sfnt/TTC face, declared post-8.5 MacRoman, Rustybuzz Latin/LTR shaping, fixed optional features, opt-in kerning, genuine glyph paths; approximate-only |
| Classic outline/multibyte/color text, synthetic FOND styles, script-specific justification | Unsupported; no silent substitution; scaling-disabled nonunit ratios and nonpositive ratios fail |
| Classic screen emulation | Not implemented |

### Standard opcode inventory

This groups **every named core opcode** in Apple's Appendix A, not merely
opcodes whose lengths can be skipped. Hex ranges below include only the listed
endpoints/ranges, not intervening reserved codes. "Implemented" means executable
portable behavior under the restrictions above; it does not mean all styles,
modes, device states or historical edge rules are supported.

| Core opcode(s) | Executable coverage / boundary |
| --- | --- |
| `00` NOP; `11` VersionOp; `FF` OpEndPic; `0C00` HeaderOp | Parsed structure/no drawing; initial version and required v2 header checked, not accepted as arbitrary midstream state changes |
| `01` Clip; `02` BkPat; `07`-`0C` pen/pattern/OvSize/Origin | Implemented ordered state; no implicit device/global pattern phase |
| `03`-`06`, `0D`, `10`, `15`-`16` text state | Implemented exact supplied-font binding and portable layout; unsupported combinations fail at text use |
| `0E`-`0F`, `12`-`14`, `1A`-`1F` color/pixel-pattern/highlight/OpColor state | Implemented; explicit highlight/OpColor needed where not recorded; restricted PixMap layouts |
| `20`-`23` Line/LineFrom/ShortLine/ShortLineFrom | Implemented; diagonal pens approximate, source-only preserved frames use shared pixel coverage |
| `28`-`2B` LongText/DHText/DVText/DHDVText | Implemented with supplied NFNT/FONT or explicit portable MacRoman TrueType binding; no inferred encoding/font |
| `2C` fontName | Retained metadata, deliberately not font discovery or substitution |
| `2D` lineJustify; `2E` glyphState | Known records parsed; nonzero justification and PreserveGlyph unsupported; other flags depend on font/layout profile |
| `30`-`34`, `38`-`3C` rectangles | All five verbs and same-rectangle forms implemented |
| `40`-`44`, `48`-`4C` rounded rectangles | All five verbs and same-bound forms implemented, approximate |
| `50`-`54`, `58`-`5C` ovals | All five verbs and same-bound forms implemented, approximate |
| `60`-`64`, `68`-`6C` arcs | All five verbs and same-bound forms implemented, approximate; each arc has its own start/sweep |
| `70`-`74` polygons; `80`-`84` regions | All five verbs implemented; polygons/complex frames approximate |
| `78`-`7C`, `88`-`8C` same polygon/region | Unsupported, not silently repeated; Apple itself labels these ten entries "Not yet implemented" |
| `90`-`91`, `98`-`99`, `9A`-`9B` Bits/PackBits/DirectBits rectangle/region | Implemented constrained mono/indexed/direct PixMaps, packing and compositing described above |
| `A0`-`A1` ShortComment/LongComment | Retained, not executed printer/PostScript semantics; strict rejects, approximate explicitly warns |
| Other reserved ranges with documented byte lengths | Retained/skipped with diagnostics, not advertised as implemented drawing; `17`-`19` lack determined sizes and fail |
| Later `8200` / `8201` QuickTime extensions | `8200` supported still profile below; `8201` retained but unsupported |

Thus the common core drawing families are present; the remaining standard-opcode
gaps are specific text-state/comment semantics and the appendix's unimplemented
same-poly/region entries, plus restricted operand/layout/mode combinations.
Genuine vector support is a **second** gate: destination-dependent drawing and
premultiplied image alpha require explicit whole-picture raster fallback, never
an isolated transparent overlay.

### Common still-image variant inventory

| FourCC / variant | Implemented | Explicitly outside current profile |
| --- | --- | --- |
| `raw ` | Interleaved RGB555/RGB24/ARGB32; explicit-palette packed 1/2/4/8-bit indices, even-byte rows | Grayscale depth codes, default/resource palettes, planar layouts, other depths |
| `rle ` Animation | Self-contained 1/2/4/8/16/24/32-bit frames; explicit palettes; packed group literals/repeats, monochrome pair controls; header 0/full header 8 | Skipped visible pixels, partial/unchanged frames, unknown header/size flags, grayscale depth codes, implicit palettes |
| `rpza` Apple Video | Self-contained RGB555 solid/interpolated/literal blocks; depth 16 or 24 descriptors | Previous-frame skips, unknown variants |
| `smc ` Apple Graphics | Self-contained 8-bit indexed blocks, explicit palette, initialized same-frame caches/repeats | Previous-frame skips, absent caches, default/resource palettes |
| `jpeg` | Established decoder, 8-bit RGB/grayscale | CMYK and higher precision |
| `png ` | Established decoder, expanded 8-bit RGB/grayscale/indexed PNG with optional alpha from PNG data | APNG and 16-bit channels |
| `cvid`, `tiff`, `gif ` and other codec IDs | No decoder advertised | Unsupported; add only explicit still/layout contracts with bounded decoder evidence |
| Shared image-description/placement | Base description plus supplied table, one frame, source crop/mask, supported affine/alpha | Separate PICT matte, extended descriptions (including zero terminator), perspective and other transfer modes |

This is a documented-format inventory, **not** a measured frequency ranking;
no representative user corpus has been supplied. Work priorities are below.

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
`tests/containers.rs`, `tests/quicktime.rs`, `tests/font_resources.rs` and `scripts/test_documents.py` are
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
Text cases cover all eight Boolean truth tables, foreground/background arithmetic,
mask/background behavior, overlapping glyphs, fractional NFNT advance selection,
signed space fractions, scaling/shrinking, negative positions, pen/origin/clip
interactions, optional table extents and excluded-ink rejection. PDFium comparisons
also cover nine source-only text/mask modes with nonunit ratios, fractional
advances and explicit fonts; destination-dependent text is checked using the
explicit whole-picture raster policy. Expected pixels are authored from the
documented portable rules, not captured from a classic system.
This is independent output-backend evidence, not independent QuickDraw fidelity.
Raw/Animation fixtures exercise scanline padding, channels, repeats/literals,
full-update headers, delta rejection, truncation, limits and malformed runs.
Affine cases include reflection, rotation, shear, fractional placement, source
masks and singular matrices. Alpha pixels are checked against independently
calculated straight/white-premultiplied/black-premultiplied equations.
PDFium tests compare affine image layers, origin/clipping, and shared diagonal,
polygon, arc and complex-region frame coverage. Frame tests verify actual
pixel-run paths (no generic strokes or image fallback; 8x8 pattern resources
remain allowed). Two overlapping partial-alpha layers allow a maximum two
8-bit channel levels of PDFium rounding difference; opaque cases remain exact.
Local FFmpeg 7.1.1 independently encoded authored RGB24/ARGB32/RGB555BE Animation
packets, all decoded to the input RGB pixels. This local codec check is not a
native-CI dependency or a historical decoder oracle.
`tests/legacy_codecs.rs` also covers RPZA/SMC block operations, partial edges,
SMC pair/quad/octet selectors, circular cache wrap/reset, current-image repeats,
truncated operands, palette bounds and absent entries, temporal dependencies,
alpha rejection and resource limits. `tests/fixtures/quicktime-blocks.json`
contains twelve original algorithmic images encoded and independently decoded
by FFmpeg 7.1.1, plus three authored RPZA packets independently decoded by FFmpeg.
The regeneration procedure and source pixels are specified by
`scripts/generate_block_fixtures.py`; native CI consumes the checked-in packet
and expected-RGB data without installing FFmpeg. Lossy RPZA compares against
the independent decoder's output, not falsely against uncompressed originals.
CLI PNG checks compare all fifteen reference images exactly. PDFium checks
RPZA/SMC affine reflections/shears and masked, clipped, origin-adjusted image
layers over existing drawing; SVG checks preserve surrounding geometry.
`tests/fixtures/quicktime-packed.json` adds twelve authored raw/Animation packets
(four indexed depths, literal/repeat and partial storage-unit edges) independently
decoded to RGB by FFmpeg 7.1.1. A thirteenth packet is independently **encoded**
by FFmpeg's gray Animation encoder, unchanged, then rebound to an explicit
depth-8 color table and independently decoded. Its white-zero indices are
retained; this does not enable grayscale description depths in the converter.
`scripts/generate_packed_fixtures.py` checks all reference results against the
authored input indices/colors before recording them. FFmpeg has no encoder for
the other packed depths here; those cases are decoder evidence, not independent
encoding evidence. Native Rust/CLI comparisons are exact. PDFium additionally
compares reflected/sheared, masked, clipped and origin-adjusted indexed layers
over drawing; SVG retains image layers. Malformed, truncated, undefined-palette,
temporal, run-overflow and resource-limit cases remain observable errors.
Font-resource fixtures construct original NFNT/FONT/FOND maps and AppleSingle
entries. Cases cover exact/ambiguous associations, source aliases, optional
directory bounds, truncation, duplicate pairs, width precedence/disablement,
scaled signed pair adjustments, run boundaries, unavailable scripts and resource
limits. FOND widths/kerning also pass through real CLI PNG/SVG/PDF conversion;
PDFium comparisons include clipping, origins and text masks.
Outline fixtures `portable-outline.ttf` and `portable-outline.ttc` are original
MIT-licensed geometric glyphs, generated with fontTools by
`scripts/generate_outline_fixtures.py`. They are not extracted system fonts or
historical samples; the private-use mapping uses an original box, not a logo.
`tests/outlines.rs` checks declared MacRoman characters, cmap failures,
explicit collection faces/styles, shaping/kerning/disabled ligatures, spacing,
ratios, clipping/origins, composite cycle rejection, resource bounds, XOR
overlap union, genuine curves and source-file protection. Independent PDFium
checks source-only and masked text modes with scaled/kerning-adjusted outlines;
curve tests verify cubic PDF geometry and independently rendered interior and
exterior pixels. Destination modes compare explicit whole-composite fallback.
Exact PDF/raster comparisons use integral outline edges: PDFium and binary
pixel-center coverage can choose different edge pixels at fractional boundaries,
which is an explicit presentation distinction, not an ignored test failure.

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
* [RFC 1740, AppleSingle/AppleDouble layouts](https://www.rfc-editor.org/rfc/rfc1740.html)
* [Apple, FOND layout](https://leopard-adc.pepas.com/documentation/mac/Text/Text-269.html),
  [associations](https://leopard-adc.pepas.com/documentation/mac/Text/Text-271.html),
  [widths](https://leopard-adc.pepas.com/documentation/mac/Text/Text-274.html),
  [kerning](https://leopard-adc.pepas.com/documentation/mac/Text/Text-276.html).
* [Adobe TN 0091, Macintosh FOND Resources](https://adobe-type-tools.github.io/font-tech-notes/pdfs/0091.Mac_Fond.pdf):
  52-byte header, width-table zero terminator and pair-count clarification.
  Negative version-0/1 values remain unsupported because the stated
  one's-complement convention conflicts with sign-magnitude interpretation in
  [Monobit's FOND reader](https://github.com/robhagemans/monobit/blob/master/monobit/storage/fontformats/apple/fond.py).
  Neither implementation nor font samples were copied.
* QuickDrawViewer `QuickTime.swift` (read-only format comparison): compressed
  picture fields and source-region transition layout; no implementation copied.
  Matrix fields follow QuickTime 16.16 affine and 2.30 projective conventions.
* [Apple, video sample data](https://developer.apple.com/documentation/quicktime-file-format/video_sample_data):
  raw scanline alignment, packed palette indices and RGB555/RGB24/ARGB32 channel order.
* [Apple, graphics modes](https://developer.apple.com/documentation/quicktime-file-format/graphics_modes):
  straight and white/black premultiplied alpha definitions.
* [MultimediaWiki, Apple QuickTime RLE](https://wiki.multimedia.cx/index.php/Apple_QuickTime_RLE):
  Animation header, row runs, skips and packed/direct formats. Its older
  [text description](https://multimedia.cx/qtrle.txt) explicitly withdrew
  inaccurate 1/2-bit details; the generic byte-depth row grammar must not be
  applied to monochrome data. The monochrome row-start bit and pair controls
  are corroborated by the [FFmpeg 7.1.1 decoder's format comments](https://github.com/FFmpeg/FFmpeg/blob/n7.1.1/libavcodec/qtrle.c)
  and [monochrome row-start regression report](https://trac.ffmpeg.org/ticket/226).
  FFmpeg independently decodes all packed fixtures and encodes the 8-bit group
  fixture. No codec implementation code or historical samples are copied.
* [Apple, video sample description](https://developer.apple.com/documentation/quicktime-file-format/video_sample_description)
  and [color table ID](https://developer.apple.com/documentation/quicktime-file-format/video_sample_description/color_table_id):
  compression identifiers, depth and embedded-table placement. The
  [color table fields](https://developer.apple.com/documentation/quicktime-file-format/color_table_atom)
  define sequential entries, `0x8000` flags, zero reserved values and count-minus-one.
* [Roberto Togni, RPZA format description](https://multimedia.cx/rpza.txt) and
  [Mike Melanson, SMC format description](https://multimedia.cx/smc.txt):
  original published reverse-engineering descriptions of block commands,
  interpolation and cache selectors, not Apple-authored codec specifications.
  Implementations here are original; neither their sample code nor third-party
  implementation code is copied. Independently encoded/decoded fixtures
  corroborate the supported variants; unknown flags remain unsupported.
* [Apple, compressed movie matte atom](https://developer.apple.com/documentation/quicktime-file-format/compressed_matte_atom):
  insufficient to establish PICT-specific separate-matte semantics; support
  remains explicitly blocked on that narrower evidence, not on a general oracle.
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
* [Apple, text scaling parameters](https://leopard-adc.pepas.com/documentation/mac/Text/Text-147.html),
  [bitmap scaling](https://leopard-adc.pepas.com/documentation/mac/Text/Text-196.html),
  [fractional width precedence](https://leopard-adc.pepas.com/documentation/mac/Text/Text-197.html),
  [fractional-width defaults](https://leopard-adc.pepas.com/documentation/mac/Text/Text-208.html),
  [SpaceExtra](https://leopard-adc.pepas.com/documentation/mac/Text/Text-153.html)
* [Monobit NFNT reader](https://github.com/robhagemans/monobit/blob/master/monobit/storage/fontformats/apple/nfnt.py):
  independently corroborates optional-table entry counts; format research only,
  no implementation or fixtures copied.
* [QuickDrawViewer](https://github.com/wiesmann/QuickDrawViewer), Apache-2.0:
  region transition interpretation and scope comparison. Its Apple-framework
  printer-style renderer is not this project's cross-platform backend or oracle.
* [Apple's Roman mapping](https://www.unicode.org/Public/MAPPINGS/VENDORS/APPLE/ROMAN.TXT):
  post-8.5 byte-to-Unicode semantics, especially the currency/euro distinction.
* [OpenType specification](https://learn.microsoft.com/en-us/typography/opentype/spec/):
  sfnt directories, TrueType contours/composites, cmap and shaping tables.
  [Rustybuzz](https://docs.rs/rustybuzz/0.20.1/rustybuzz/) and its ttf-parser
  dependency provide portable shaping and contour extraction, not Font Manager
  emulation. Locked versions and redistribution notices travel with CLI assets.

## Readiness

This is a usable **defined supported converter**, not a fully capable historical
QuickDraw backbone. The supported contract is the matrix above plus
`portable_rgb_v1`, `portable_nfnt_v1`, `portable_outline_v1`, `portable_paths_v1`
and JSON protocol 1.
The component remains deferred. The gate is a decision about that explicit
contract, not a requirement to implement every imaginable historical extension.

| Component readiness checklist | Status / gate |
| --- | --- |
| Explicit inputs, formats, limits, exit codes, safe outputs and qualified success | Implemented; consumer must preserve originals and diagnostics |
| Font usability without platform font discovery | Implemented for supplied NFNT/FONT, explicit Roman FOND, and explicitly declared portable MacRoman/static TrueType outline bindings; no user fonts shipped |
| Source-only preserved SVG/PDF and safe destination-dependent output | Implemented subset; caller must explicitly choose whole-picture raster where preserved vectors cannot represent the operations |
| Native executable and reproducible package evidence | Three-target CI verifies macOS ARM64/x64 and Windows x64; gate each selected revision on its actual successful run and verified manifests/hashes |
| Product acceptance of the supported matrix | Coordinator/user decision remains: accept explicit unsupported outcomes and portable approximations, or identify required additional input features |
| Durable distribution, signing/notarization and release authorization | Not implemented/authorized in this milestone; unsigned expiring CI artifacts are not a release channel |
| Component protocol adapter | Deliberately untouched; must expose exact source selections, fallback policy, limits, exit codes and warnings without guessing/retrying fonts |
| Historical accuracy claims | Unavailable evidence, not an implementation prerequisite for the defined subset; no classic equivalence claim |

### Next compatibility work versus external dependencies

| Gap | What can proceed without a historical oracle | Evidence or decision still needed |
| --- | --- | --- |
| Remaining family behavior | Documented synthetic-style properties and richer metadata inspection can be implemented if added to the supported contract | Negative version-0/1 values need a resolved representation contract or identified historical samples; current exact intrinsic bindings do not synthesize styles |
| Outline fonts and encodings | Portable Roman static TrueType outlines implemented; CFF and other declared encodings can be added independently | Matching fonts and source encoding must be supplied. Classic metrics remain separate; non-Roman language/direction/shaping needs a new explicit contract |
| Justification | The declared portable Roman outline profile now provides a basis for documented Roman spacing; implement and test the exact recorded lineJustify interpretation | Non-Roman script identity remains a contract decision; a raw NFNT does not identify a Script Manager environment |
| Embedded images | Indexed/direct raw/Animation, RPZA, explicitly paletted SMC, affine placement and explicit alpha modes implemented; explicit grayscale profiles, bounded description extensions, other codecs and OpColor modes remain implementable | Separate PICT matte byte layout, coordinate mapping and mode interaction need a verified specification or provenance-backed samples; movie matte atoms are not sufficient evidence |
| Preserved vector coverage | Shared diagonal/polygon/arc/complex-region frame paths implemented; destination-dependent operations still require full-composite fallback | Editable continuous geometry versus portable pixel-run presentation remains a fidelity choice; premultiplied source-over needs a representable precision contract before preserved-vector support |
| Additional containers | Explicit native resource-fork access can build on current extraction; AppleSingle v2 is implemented | Filesystem behavior and deployment policy must be tested per platform |
| Classic fidelity | Improve specification-backed coverage and differential backend checks now | Pixel equivalence requires an identified OS/QuickDraw/device/font setup, lawful reference outputs and a representative corpus |

None of those missing oracle inputs prevents further ordinary compatibility
implementation. They do prevent claiming complete legacy behavior. Current
fractional placement and whole-run compositing remain explicitly portable;
historical chunk boundaries, cached font data, printer adjustments, global
pattern phase and implicit system colors/fonts are not reconstructed.

### Prioritized ordinary compatibility gaps after packed stills

| Priority by documented scope (not guessed corpus frequency) | Next implementable work / boundary |
| --- | --- |
| 1. Common existing-record variants | Bounded image-description extensions/optional zero terminator; explicit grayscale depth/polarity profiles; documented image OpColor blend. Require verified layouts and tests, not a new historical oracle |
| 2. Existing standard text operations | Portable Roman lineJustify and supported additional static outline formats such as CFF; retain supplied fonts, fixed encoding, spacing and feature contracts |
| 3. Additional named still codecs / containers | Evaluate portable bounded TIFF/GIF/Cinepak decoders for self-contained images; `8201` only after its specific PICT layout is established; native resource-fork access is deployment convenience |
| 4. New policies rather than guessed defaults | Default/resource image palettes require explicit identification/provenance; printer comment execution needs a defined supported comment contract; color management/device behavior is separate |

Temporal files cannot become self-contained stills without supplying their
previous frames. The undefined same-poly/region entries are not a reason to
invent QuickDraw behavior or block a decision about the existing supported set.
The separate-matte blocker is specifically the PICT payload layout, coordinate
mapping, coverage/polarity and transfer-mode interaction, not movie `kmat`
atom knowledge or a general demand for a historical oracle.

The user selected portable outline text first, with classic fidelity separate.
That decision is now implemented as the explicit `portable_outline_v1` contract
in the CLI documentation, without changing NFNT byte-addressed behavior. Matching
lawfully usable fonts and declared MacRoman input are still required. Remaining
outline implementation gaps are CFF/variable/color fonts, font resource-container
selection, synthetic faces and richer layout. Non-Roman scripts and pre-8.5
currency-sign mappings need explicit encoding/language/direction contracts before
support; classic Macintosh sfnt mapping and metrics are not claimed by this API.
