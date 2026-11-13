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
| Indexed PixMaps in v2 | 1/2/4/8-bit; address versions 0/4; color-table index or sequential device-table entries; absent/duplicate indices and unknown flags rejected |
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
| Printer comments (`A0`, `A1`) | Default unchanged: strict fails, approximate warns/ignores. Opt-in `apple-portable` supports bounded text rotation and multiplicative fractional axis-aligned line widths; never executes PostScript |
| Origin (`0C`) | Horizontal/vertical delta; local clip/pen/last-shape state preserved; prior pixels do not move |
| Oval size / rounded rectangles (`0B`, `40` family) | Approximate elliptical corners, clamped diameters, inset framing |
| Arcs (`60` family) | Approximate elliptical sectors; zero at 12 o'clock, positive clockwise; signed sweeps; frame excludes radii |
| Pixel patterns (`12`-`14`) | Bit, solid RGB and indexed color tiles; bounded shared PixMap decoding; color patterns copy directly without pen-mode/fg/bg mapping |
| OpColor / arithmetic modes (`1F`, modes 32-39) | Component blend, pin/wrap add/subtract, transparency, max/min; blend and pin modes require explicit OpColor |
| Highlight (`1C`-`1E`, modes 50/52/58/60) | Exchanges background/highlight colors, leaves other destination colors intact; one-shot HiliteMode; explicit HiliteColor or CLI highlight required |
| Transparent pattern mode 44 | Whole-pixel background keying |
| CopyBits dither hint 64-71 | Approximate-only inactive hint when no source precision or spatial reduction occurs; underlying source mode preserved, explicit diagnostic; actual dithering/other flag combinations unsupported |
| QuickTime compressed (`8200`) | JPEG/PNG, single-page TIFF/single-image GIF, direct and explicitly paletted 1/2/4/8-bit raw/Animation, RGB555 RPZA and explicitly paletted SMC self-contained stills; exact explicit boundaries in CLI contract |
| QuickTime affine/alpha | Nonsingular affine rotation/reflection/shear/fractional placement; straight and premultiplied white/black alpha, explicit approximate policy; source mask, clip and origin preserved |
| QuickTime grayscale/description/OpColor | Explicit white-zero depths 34/36/40; retained neutral fiel/pasp/clap and optional zero terminator; modes 32/260 with recorded OpColor and approximate policy |
| QuickTime uncompressed (`8201`) / other codecs | Explicitly unsupported; no silent substitute decoder |
| Unknown opcodes / unknown length layouts | Fail with offset and opcode; no recovery by guessing |
| AppleSingle/AppleDouble v1 / packed MacBinary / native resource-fork filesystem access | Unsupported; supply a supported file container |
| PNG | Opaque RGB output |
| SVG / PDF | Explicit whole-picture raster or supported genuine-path policy; PDF 1.4 single page |
| Native vector subset | Tiled patterns, copied/straight-alpha image layers, shapes/regions and NFNT runs; diagonal/polygon/arc/complex-region frames use shared pixel-run coverage |
| Remaining embedded-image gaps | Separate matte payloads, projective matrices, other transfer modes, semantic extensions, temporal/skipped-pixel Animation, implicit color palettes, planar raw, CMYK/16-bit-channel images; premultiplied alpha and OpColor blending need whole-picture raster |
| Portable outline text | Explicit static TrueType or supported name-keyed CFF1 sfnt/TTC face, declared post-8.5 MacRoman, Rustybuzz Latin/LTR shaping, fixed optional features, opt-in kerning, genuine glyph paths; approximate-only |
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
| `28`-`2B` LongText/DHText/DVText/DHDVText | Implemented with supplied NFNT/FONT or explicit portable MacRoman TrueType/CFF1 binding; no inferred encoding/font |
| `2C` fontName | Retained metadata, deliberately not font discovery or substitution |
| `2D` lineJustify; `2E` glyphState | Known records parsed; nonzero justification and PreserveGlyph unsupported; other flags depend on font/layout profile |
| `30`-`34`, `38`-`3C` rectangles | All five verbs and same-rectangle forms implemented |
| `40`-`44`, `48`-`4C` rounded rectangles | All five verbs and same-bound forms implemented, approximate |
| `50`-`54`, `58`-`5C` ovals | All five verbs and same-bound forms implemented, approximate |
| `60`-`64`, `68`-`6C` arcs | All five verbs and same-bound forms implemented, approximate; each arc has its own start/sweep |
| `70`-`74` polygons; `80`-`84` regions | All five verbs implemented; polygons/complex frames approximate |
| `78`-`7C`, `88`-`8C` same polygon/region | Unsupported, not silently repeated; Apple itself labels these ten entries "Not yet implemented" |
| `90`-`91`, `98`-`99`, `9A`-`9B` Bits/PackBits/DirectBits rectangle/region | Implemented constrained mono/indexed/direct PixMaps, packing and compositing described above |
| `A0`-`A1` ShortComment/LongComment | Retained; explicit printer subset for 150/151/154/182 and empty 190/191 synchronization. Other semantics rejected in printer profile; default behavior unchanged |
| Other reserved ranges with documented byte lengths | Retained/skipped with diagnostics, not advertised as implemented drawing; `17`-`19` lack determined sizes and fail |
| Later `8200` / `8201` QuickTime extensions | `8200` supported still profile below; `8201` retained but unsupported |

Thus the common core drawing families are present; the remaining standard-opcode
gaps are specific text-state/comment semantics and the appendix's unimplemented
same-poly/region entries, plus restricted operand/layout/mode combinations.
Genuine vector support is a **second** gate: destination-dependent drawing and
premultiplied image alpha require explicit whole-picture raster fallback, never
an isolated transparent overlay.

### Core PixMap/CopyBits operand inventory

No additional classic pixel depth or row-packing algorithm was missing from the
baseline. The bounded extension accepts the documented address-only version 4,
adds only an explicitly inactive dither-hint profile, and stops silently ignoring
unknown flags/reserved fields. It does not invent a 24-bit classic PixMap layout
from QuickTime's separate 24-bit format.

| Operand | Executable coverage | Explicit rejection / boundary |
| --- | --- | --- |
| BitMap | 1-bit MSB-first; foreground/background expansion; v1/v2 | PixMap records in v1; invalid odd/zero/too-small rowBytes |
| Indexed PixMap | MSB-first 1/2/4/8 bits, pixelType 0, one component of pixelSize bits | Other depths/types/component layouts; absent used indices; no implicit device palette |
| Direct PixMap | pixelType 16, RGB555 with 3x5 bits; RGB32 with 3/4x8 bits, high byte padding | 24-bit pixelSize, alpha interpretation of padding, unsupported component tuples/planar memory |
| Indexed/mono packing | Unpacked short rows; byte PackBits for packed opcodes; indexed packType 1 explicitly unpacked | Indexed packType >1; BitsRect/BitsRgn with rowBytes >=8 (both PICT versions, Appendix A footnotes 5/8) |
| Direct packing | 16-bit types 1/3; 32-bit types 1/2/4; type 0 defaults to 3/4; rows <8 bytes uncompressed even when another legal packing type is recorded | Other depth/packing pairs and unknown types; bad stride/component count |
| PackBits framing | Signed controls, -128 no-op, byte/word units; exact row output; count is u16 iff original rowBytes >250, otherwise u8 | Row overrun/underrun, truncated controls/data; bounded input, decoded bytes and work |
| Color tables | ctFlags 0: ColorSpec.value is index; 0x8000: supplied table order; RGB16 high bytes under existing RGB8 profile | Other ctFlags; duplicate/out-of-range indices; no active GDevice table, color-search callback or cached inverse table |
| Other fields | pmVersion 0/4, zero planeBytes/pmReserved/packSize; pmTable/baseAddr addresses never dereferenced, cache seed is not a missing-palette authorization | Private version bits, nonzero reserved/planar fields; nonzero PICT packSize malformed; DirectBits sentinel still required |
| Source transfer | 0-7; existing arithmetic 32-39 and highlight 50/52/58/60 with explicit required state | Other base modes; absent OpColor/highlight; pattern-only modes not accepted as CopyBits sources |
| Dither flag | 64-71 reduce to their corresponding 0-7 source modes only under approximate policy and the no-reduction RGB8 conditions below | Shrink in either axis, higher-precision palettes, other combined flags; no actual dithering or device palette quantization |

The inactive-hint source profile permits monochrome, RGB555/RGB32 and indexed
tables whose **every supplied** RGB16 component equals replicated RGB8.
Source/destination rectangles must not shrink either dimension. It emits
`copybits_dither_inactive` rather than silently discarding bit 64, leaves the
recorded mode intact internally, and shares effective modes across raster and
vector. Only 64/68 are source-only preserved-vector cases; destination-dependent
variants need the existing complete-composite raster policy. Ordinary mode 36
continues to key against background, unlike QuickTime's separate OpColor key.
Unflagged shrinking retains the existing declared nearest-neighbor profile;
historical direct-device shrink averaging is **not** newly claimed.

Sources: Apple's *Inside Macintosh: Imaging With QuickDraw*, HTML revision
7 July 1996: [PixMap](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-202.html),
[ColorSpec](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-205.html),
[ColorTable](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-206.html),
[Appendix A packing](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-461.html),
[CopyBits](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-166.html), and
[dithering/device behavior](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-199.html).
The flag, address and packing definitions are direct documentary evidence.
The no-reduction acceptance rule is an explicit conservative **portable policy**,
not a recorded historical rendering result or recovered dithering algorithm.

`scripts/generate_pixmap_fixtures.py` writes 13 original fixtures in
`tests/fixtures/pixmaps.json`; Pillow 11.1.0/libtiff 4.6.0 independently encode
and decode each byte-PackBits row. Authored PICT framing/expected colors cover
all indexed depths, reordered/sequential tables, padding, prefix threshold
250/252 and direct 3/4-component packing including short-row override.
RGB555 word-unit PackBits remains separately source-authored, not claimed as
libtiff-encoded. Tests cover truncations/budgets, unknown flags, PixPat reuse,
strict rejection, all accepted flagged source modes, scaling/origin/clip and
overlap. Pillow checks decoded PNG/SVG layer pixels; PDFium independently checks
masked/clipped PDF rendering and complete-composite fallback. None is a
classic-Mac fidelity oracle.

### Remaining gaps after the core bitmap milestone

| Category | Remaining gap | What permits further work |
| --- | --- | --- |
| Core standard operations | Nonzero lineJustify, PreserveGlyph/other excluded text state; Apple's undefined same-poly/region slots; other HeaderOp source mappings | Affirmative playback/mapping evidence for the particular operand; do not infer behavior from reserved opcode lengths |
| Core device-dependent CopyBits | Actual dithering, indexed destination palette search, shrink averaging, higher-precision flagged palettes and other mode combinations | Explicit destination precision/palette/filter policy or an identified historical device target; current defaults are unchanged |
| Optional extensions | Separate PICT mattes, QuickTime 8201/projective/color-managed images and other codecs; broader printer/CFF/font/script profiles | PICT-specific matte/layout evidence, or explicitly selected supported contracts; non-Roman needs encoding/language/direction; not automatic blockers for the current subset |
| Unavailable fidelity evidence | Classic edge rules, global/device state, font metrics and producer-specific equivalence | Lawful source/output pairs with OS/QuickDraw/device/font identities and representative real files |

No further missing documented **device-independent PixMap pixel/packing family**
was identified. More core regression/producer-bug work can proceed under the
existing contract; no further rendering feature is asserted implementable from
this inventory without the specific evidence or policy above. The known
lineJustify/matte investigations were not reopened. Component acceptance,
signed distribution and release publication remain separate, deferred gates.

### Common still-image variant inventory

| FourCC / variant | Implemented | Explicitly outside current profile |
| --- | --- | --- |
| `raw ` | Interleaved RGB555/RGB24/ARGB32; explicit-palette packed 1/2/4/8-bit indices; explicit 34/36/40 white-zero grayscale depths; even-byte rows | Default/resource color palettes, depth 33, planar layouts, other depths |
| `rle ` Animation | Self-contained 1/2/4/8/16/24/32-bit frames and 34/36/40 grayscale; explicit color palettes; packed groups/monochrome pairs; header 0/full header 8 | Skipped visible pixels, partial/unchanged frames, unknown flags/depths, implicit color palettes |
| `rpza` Apple Video | Self-contained RGB555 solid/interpolated/literal blocks; depth 16 or 24 descriptors | Previous-frame skips, unknown variants |
| `smc ` Apple Graphics | Self-contained 8-bit indexed blocks, explicit palette, initialized same-frame caches/repeats | Previous-frame skips, absent caches, default/resource palettes |
| `jpeg` | Established decoder, 8-bit RGB/grayscale | CMYK and higher precision |
| `png ` | Established decoder, expanded 8-bit RGB/grayscale/indexed PNG with optional alpha from PNG data | APNG and 16-bit channels |
| `tiff` | Classic II/MM, one IFD, strips, chunky top-left 8-bit gray/RGB/unassociated RGBA; none/PackBits/LZW/Deflate; predictor 1/2 | Multi-page/SubIFD/BigTIFF, tiles, palette TIFF, CMYK/Lab, other precision/layout/compression, associated alpha, ICC/color semantics and unknown tags |
| `gif ` | One full-screen image, global/local palette, interlacing, transparent index, disposal 0/1 | Animation, partial frames, loop/application/plain-text extensions, user-input/disposal dependency, missing palette, non-square aspect |
| `cvid` and other codec IDs | No decoder advertised | Unsupported; add only explicit still/layout contracts with bounded decoder evidence |
| Shared image-description/placement | Base description/table, neutral progressive/square/full-centered hints, optional final zero terminator; source crop/mask, affine/alpha, recorded OpColor blend and exact RGB8 transparency key | Separate PICT matte, gamma/color-management/nonneutral or unknown extensions, perspective and other transfer modes |

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
retained rather than inferred from palette color values.
`scripts/generate_packed_fixtures.py` checks all reference results against the
authored input indices/colors before recording them. FFmpeg has no encoder for
the other packed depths here; those cases are decoder evidence, not independent
encoding evidence. Native Rust/CLI comparisons are exact. PDFium additionally
compares reflected/sheared, masked, clipped and origin-adjusted indexed layers
over drawing; SVG retains image layers. Malformed, truncated, undefined-palette,
temporal, run-overflow and resource-limit cases remain observable errors.
The same fixture corpus now has **23 cases**: nine additional authored
raw/Animation depth-34/36/40 packets, carrying neutral description extensions
and terminators, independently decode in FFmpeg to the specified white-zero
ramps; one unmodified FFmpeg depth-40 encoder output provides independent
grayscale encode/decode evidence. Extension framing tests cover every truncated
boundary, malformed rationals/sizes, duplicate atoms, unknown semantic fields,
exact inspection retention and operation limits.
Python's independent rational-arithmetic reference checks OpColor blend and
straight-alpha blend across weight/alpha endpoints and near-endpoints; PDFium
checks whole-composite fallback with source masks, clipping and origin changes.
This is mathematical/decoder/backend evidence, not historical ICM output.
Font-resource fixtures construct original NFNT/FONT/FOND maps and AppleSingle
entries. Cases cover exact/ambiguous associations, source aliases, optional
directory bounds, truncation, duplicate pairs, width precedence/disablement,
scaled signed pair adjustments, run boundaries, unavailable scripts and resource
limits. FOND widths/kerning also pass through real CLI PNG/SVG/PDF conversion;
PDFium comparisons include clipping, origins and text masks.
Outline fixtures `portable-outline.ttf`, `portable-outline.ttc`,
`portable-outline-cff.otf` and `portable-outline-mixed.ttc` are original
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
CFF fixtures contain actual cubic curves, local/global subroutines, stems/masks
and an empty space. fontTools independently decompiles the compiled font into
`portable-outline-cff.json`; document tests compare those cubic controls with
PDF/SVG paths and compare raster coverage with an independent cubic equation.
PDFium checks CFF interior/exterior samples, spacing, origin/clip, source and
destination modes, including the selected CFF face in a mixed TTC. Malformed
INDEX/DICT/mask/program tests and shallow exponentially expanded subroutine tests
exercise allocation, nesting and total-work limits. Existing TrueType fixture
bytes and behavior are preserved. No third-party glyph data is copied.

The tests' truth tables and selected expected pixels establish the declared
implementation profile, **not independent rendering fidelity**. The mutation
sweep is a bounded deterministic regression test, not exhaustive fuzzing.
The archive tests use deliberately synthetic PE/Mach-O headers only to test
identity checks; real native artifacts must separately pass the packaging gate.

No classic-Mac emulator, historical fonts, printer environment, real-world user
corpus or independent pixel oracle was supplied. Accordingly no validation of
pixel-perfect classic output, text layout or general printer output is claimed.

TIFF/GIF fixtures in `quicktime-stills.json` are original geometric images,
independently encoded and decoded by Pillow 11.1.0 / libtiff 4.6.0 using
`scripts/generate_still_fixtures.py`. Twenty valid cases cover every advertised
compression, both TIFF byte orders, multiple strips, white-zero gray, prediction,
RGBA, GIF local/global palettes, transparency and actual interlacing; six
additional cases are explicit unsupported outcomes. Tests exercise complete
truncation sweeps, excessive decompressed output, missing LZW/zlib endings,
palette failures, page/frame ambiguity, metadata, dimensions and budgets.
PDFium compares masked/scaled/rotated alpha layers and whole-composite OpColor
fallback; SVG embedded PNG pixels are independently compared after placement
and source masking. These are decoder/backend checks, not QuickTime fidelity.
ImageMagick and QuickDrawViewer are not used as authoritative fidelity oracles.
Native CI results must be read from actual workflow runs; merely adding Windows
jobs is not evidence that Windows tests passed.

Printer/key cases use original PICT records and the existing lawful NFNT,
TrueType and CFF fixtures. Tests distinguish RGB-triple OpColor key equality
from channel blending and ordinary background keying, including low-byte
quantization and ordered key changes. Rotation checks first-baseline and
reanchored centers, fractional angles/offsets, axis flips, default-versus-printer
selection, origin outside groups, ignored/restored clipping, and suppressed
fallback bitmaps. Fractional-width tests exercise accumulated/inverse factors,
PenSize independence, exact vector extents and bounded ratios. Malformed groups,
unsupported layouts/comments and transformed-path budgets fail explicitly.
PDFium and embedded SVG-image checks independently verify key masks, rotated
TrueType/CFF geometry and full-composite XOR fallback. Fractional lines are
also rendered at higher PDF resolution to confirm real fractional width, not
integer pen rounding. Native raster remains pixel-center coverage; edge pixels
and subpixel hairlines can differ from continuous vector presentation.

## References (format research, not copied implementation)

* [Apple, picture version/header opcodes](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-459.html)
* [Apple, opcode data types](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-460.html)
* [Apple, PICT opcode and PixMap packing tables](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-461.html)
* [Apple, QuickDraw image/pen/shape semantics](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-12.html)
* [Apple, printer comments](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-465.html)
  (Inside Macintosh: Imaging With QuickDraw, Appendix B; HTML dated 7 July 1996).
  [Text rotation](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-469.html)
  establishes signed clockwise integer/Fixed rotation, first-text center
  anchoring, clipping/CopyBits suppression and bitmap fallback separation.
  Its Fixed field declaration controls the ten-byte form, not the contradictory
  `Byte` declaration in the appendix's constants listing.
  [Fractional widths](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-471.html)
  explicitly distinguishes multiplicative LaserWriter and replacement SC state.
  [Synchronization](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-468.html)
  establishes empty 190/191 flush pairs. No example implementation is copied.
  These establish a limited opt-in interpretation, not device-equivalence evidence.
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
* [Apple, description extensions](https://developer.apple.com/documentation/quicktime-file-format/video_sample_description_extensions),
  [pixel aspect ratio](https://developer.apple.com/documentation/quicktime-file-format/pixel_aspect_ratio)
  and [clean aperture](https://developer.apple.com/documentation/quicktime-file-format/clean_aperture):
  bounded suffix framing and neutral progressive/square/full-image profiles.
  Gamma/color semantics are not ignorable metadata. The sample-description
  definition documents the optional final four-byte zero terminator.
* [Apple, depth codes](https://developer.apple.com/documentation/quicktime-file-format/video_sample_description/depth)
  and [grayscale ramps](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-198.html):
  explicit 2/4/8-bit grayscale and linear white-to-black levels; FFmpeg
  independently verifies this polarity for raw/Animation description depths.
* [Apple, alpha channels](https://preterhuman.net/macstuff/techpubs/quicktime/qtdevdocs/REF/refImageCompMgr.7.htm)
  and [QuickDraw blend formula](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-199.html):
  source alpha, component OpColor weights and straightAlphaBlend; quantization
  remains the explicitly documented portable profile.
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
* Adobe [CFF specification #5176](https://adobe-type-tools.github.io/font-tech-notes/pdfs/5176.CFF.pdf)
  and [Type 2 charstrings #5177](https://adobe-type-tools.github.io/font-tech-notes/pdfs/5177.Type2.pdf):
  INDEX/DICT layout, subroutine bias/calls, widths, masks, stacks and cubic operators.
  [OpenType CFF](https://learn.microsoft.com/en-us/typography/opentype/spec/cff)
  specifies single-font Type 2 tables and matching glyph counts.
* [TIFF 6.0](https://download.osgeo.org/libtiff/doc/TIFF6.pdf), Adobe, June 1992:
  classic IFDs, strips, photometric/sample/extra-sample tags, LZW, PackBits and
  horizontal prediction. Deflate uses the interoperable zlib stream convention.
  [GIF89a specification](https://www.w3.org/Graphics/GIF/spec-gif89a.txt),
  CompuServe, July 1990: logical screens, palettes, graphic controls, image
  descriptors, sub-blocks and LZW termination. Pinned Rust `tiff`, `gif`, `weezl`
  and `flate2` provide established portable decoding; no implementation code
  from Pillow/libtiff is copied.
* [QuickDraw Appendix A footnote 2](https://dev.os9.ca/techpubs/mac/QuickDraw/QuickDraw-461.html)
  identifies lineJustify's extra-character and total-style-run Fixed operands.
  [DrawJustified](https://preterhuman.net/macstuff/techpubs/mac/Text/Text-160.html)
  describes script-dependent slop and temporary SpExtra additions. Neither
  establishes the missing recorded-state replay mapping described in the CLI
  contract; nonzero Roman lineJustify remains unsupported rather than guessed.

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
| Font usability without platform font discovery | Implemented for supplied NFNT/FONT, explicit Roman FOND, and explicitly declared portable MacRoman/static TrueType or supported CFF1 outline bindings; no user fonts shipped |
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
| Outline fonts and encodings | Portable Roman static TrueType and name-keyed Type 2 CFF1 implemented; CID/seac, variable/color fonts and other explicit encodings remain optional expansions | Matching fonts and source encoding must be supplied. Classic metrics remain separate; non-Roman language/direction/shaping needs a new explicit contract |
| Justification | Existing explicit spacing/scaling works; nonzero lineJustify is still rejected even for Roman | Need authoritative playback interaction of extra-character/total-run operands with recorded SpExtra/ChExtra, scaling, trailing spaces and state lifetime, or provenance-backed recording/output pairs. Bare NFNT has no inferred script |
| Embedded images | Indexed/direct/grayscale raw/Animation, RPZA, SMC, single-page TIFF/single-image GIF, neutral description extensions, affine/alpha and OpColor blend implemented; other codecs and nonneutral presentation remain work | Separate PICT matte byte layout, coordinate mapping and mode interaction need a verified specification or provenance-backed samples; movie matte atoms are not sufficient evidence |
| Preserved vector coverage | Shared diagonal/polygon/arc/complex-region frame paths implemented; destination-dependent operations still require full-composite fallback | Editable continuous geometry versus portable pixel-run presentation remains a fidelity choice; premultiplied source-over needs a representable precision contract before preserved-vector support |
| Additional containers | Explicit native resource-fork access can build on current extraction; AppleSingle v2 is implemented | Filesystem behavior and deployment policy must be tested per platform |
| Classic fidelity | Improve specification-backed coverage and differential backend checks now | Pixel equivalence requires an identified OS/QuickDraw/device/font setup, lawful reference outputs and a representative corpus |

None of those missing oracle inputs prevents further ordinary compatibility
implementation. They do prevent claiming complete legacy behavior. Current
fractional placement and whole-run compositing remain explicitly portable;
historical chunk boundaries, cached font data, printer adjustments, global
pattern phase and implicit system colors/fonts are not reconstructed.

### Prioritized ordinary compatibility gaps after key/printer support

| Priority by documented scope (not guessed corpus frequency) | Next implementable work / boundary |
| --- | --- |
| 1. Remaining existing-record variants | Non-square pixel/clean-aperture presentation under an explicit PICT mapping contract, and color-management/gamma handling. Neutral suffixes, explicit grayscale, image blend and OpColor key (36) are implemented |
| 2. Existing standard text operations | Nonzero Roman lineJustify is blocked on the specific replay evidence above, not on CFF or general historical accuracy. PreserveGlyph/extended text state and additional CFF profiles require their own documented supported semantics |
| 3. Additional named still codecs / containers | TIFF/GIF single-image profiles are implemented; Cinepak and additional TIFF variants remain optional work. `8201` only after its specific PICT layout is established; native resource-fork access is deployment convenience |
| 4. New policies rather than guessed defaults | Default/resource image palettes require explicit identification/provenance; printer support is the opt-in text/axis-line subset, not arbitrary comment execution; color management/device behavior is separate |

The finite printer milestone covers 150/151/154 text groups with no alignment,
unmasked ink-only text and exact supplied fonts, 182 multiplicative positive
widths for axis-aligned lines, and empty 190/191 synchronization pairs.
Remaining printer implementation variants are aligned/background/masked text,
diagonal/shape-frame fractional pens, line/string accumulation, nontext
suspend/resume and other comment families. Combined flip/rotation order and
stored-center remapping under an in-group origin change are deliberately not
inferred from the simple Appendix B examples; those require a declared mapping
or discriminating producer/output evidence before expanding this profile.
Arbitrary PostScript execution is not a future requirement of this converter.

Temporal files cannot become self-contained stills without supplying their
previous frames. The undefined same-poly/region entries are not a reason to
invent QuickDraw behavior or block a decision about the existing supported set.
Remaining **standard** behavior gaps are unchanged core-opcode families versus
operand semantics: nonzero lineJustify/PreserveGlyph and extended text state;
printer-comment variants beyond the explicit subset; the appendix's unimplemented same-poly/region codes;
restricted PixMap/transfer/dithering and implicit device-state combinations.
Later QuickTime gaps are listed separately above, not counted as missing basic
rectangle/line/text opcodes. Non-Roman shaping still requires a new explicit
encoding/language/direction contract; it is not silently inferred from sfnt.
The separate-matte blocker is specifically the PICT payload layout, coordinate
mapping, coverage/polarity and transfer-mode interaction, not movie `kmat`
atom knowledge or a general demand for a historical oracle.

The user selected portable outline text first, with classic fidelity separate.
That decision is now implemented as the explicit `portable_outline_v1` contract
in the CLI documentation, without changing NFNT byte-addressed behavior. Matching
lawfully usable fonts and declared MacRoman input are still required. Remaining
outline implementation gaps are CID/seac and other excluded CFF variants,
CFF2/variable/color fonts, font resource-container
selection, synthetic faces and richer layout. Non-Roman scripts and pre-8.5
currency-sign mappings need explicit encoding/language/direction contracts before
support; classic Macintosh sfnt mapping and metrics are not claimed by this API.

**Consolidated gate:** functional bounded ingestion, raster output, explicit
text/image profiles and preserved SVG/PDF plus opt-in full-composite fallback
are implemented. Missing lineJustify replay evidence and separate PICT matte
layout are concrete compatibility blockers **for files using them**; neither is
silently approximated. Classic device/font equivalence needs identified lawful
reference outputs and a representative corpus. Additional codecs, CID fonts,
printer comments and color management are feature-scope choices, not automatic
blockers for accepting the defined subset. Product acceptance, durable signed
distribution and the component adapter remain separate coordinator/user gates;
this milestone does not authorize a component or release.
