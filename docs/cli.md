# CLI contract: protocol 1

Executable: `quickdraw` on macOS; `quickdraw.exe` on Windows.
Use an argument array with `4D.SystemWorker`, not a concatenated shell command.
Paths must be valid UTF-8; spaces, Unicode and shell metacharacters are supported.
There is no stdin/stdout image mode, batch mode, implicit recursive traversal,
network fetch, font substitution, or side-effecting configuration.

## Commands

```text
quickdraw --version
quickdraw capabilities
quickdraw list-pictures --input PATH [INPUT OPTIONS]
quickdraw list-fonts --input PATH [INPUT OPTIONS]
quickdraw inspect --input PATH [INPUT OPTIONS] [SELECTION OPTIONS]
quickdraw convert --input PATH --output PATH [INPUT OPTIONS] [CONVERSION OPTIONS]
quickdraw --help
quickdraw convert --help
quickdraw inspect --help
```

`--version` must stand alone and emits `{"protocol":1,"version":"0.1.0"}`.
`capabilities` returns formats, containers, supported and unsupported semantics,
default limits and exit codes. Help is human-readable text on stdout, exit 0.
All other commands use the JSON contract below. There is deliberately no inert
`--json`, system-font-search, or pixel-perfect switch.

### Input options (list-pictures, list-fonts, inspect and convert)

| Argument | Default | Behavior |
| --- | --- | --- |
| `--input PATH` | required | One regular file; never modified |
| `--container auto\|raw\|file\|resource-fork\|macbinary\|appledouble\|applesingle` | `auto` | Explicit layout or unambiguous header detection; no arbitrary signature scan |
| `--max-input-bytes N` | 67108864 | Maximum file bytes, checked before and during reading |
| `--max-pixels N` | 16000000 | Maximum frame, bitmap, region, and output pixel count, individually |
| `--max-decoded-bytes N` | 134217728 | Cumulative decoded/scratch allocation accounting per parsing/rendering/vector phase; also bounds aggregate loaded font bytes |
| `--max-operations N` | 100000 | Maximum parsed operations (excluding header and EndPic); also bounds container tables/resources, codec framing, TIFF entries/strips and GIF blocks |
| `--max-work N` | 100000000 | Parser input/decoding and renderer coverage work budgets, separately |

Every limit must be positive. Increasing them is an explicit resource decision.
Work measures deterministic implementation work, not elapsed seconds. Limits do
not promise a hard process RSS cap: vector capacities, diagnostics, serialized
inspection output, PNG compression and base64 encoding require additional
memory. Use an external process memory limit and timeout for untrusted batches.
Parsing and rendering have separate budgets; a successful parse can still exceed
the rendering budget. Whole-picture rendering precedes output resampling, so a
small requested output does not bypass the native frame limit.

An extracted resource payload is `raw`, not a resource-fork map. A data-fork PICT
file with a 512-byte header is `file`. Resource-fork maps, MacBinary I/II/III and
AppleDouble/AppleSingle v2 can contain multiple candidates; selection never silently picks
the first. Each selected PICT payload must have no trailing data.
PICT v1 nonzero `picSize` must equal consumed bytes; zero is accepted as unspecified.
PICT v2 `picSize` is ignored, per the format documentation.

### Container inventory and picture selection

```text
quickdraw list-pictures --input archive.bin
quickdraw inspect --input archive.bin --resource-id -128
quickdraw convert --input archive.bin --data-fork --output picture.png
```

`list-pictures` returns `{"protocol":1,"inventory":{"container":"macbinary",
"pictures":[...]}}`. Each entry has `source` (`resource` or `data_fork`),
`resource_id` (signed integer or null), `name_hex` (original name bytes or null),
and absolute `offset`/`length` of the PICT payload in the input. Inventory validates
container structure, not every picture or codec. Resource names are metadata,
never filesystem paths; no contained files are written.

`--resource-id ID` and `--data-fork` apply only to inspect/convert and are mutually
exclusive. Without either, exactly one candidate is required. Zero/multiple
matches fail with exit 2, leaving outputs unchanged. IDs are not ordinal indexes;
duplicate IDs within a resource type fail as malformed. A MacBinary file can
contain both a data-fork PICT and PICT resources. `--container` disambiguates the
outer layout; the selection flags disambiguate its picture.

Resource map/data/reference/name ranges and overlaps are checked. MacBinary II/III
header CRCs, fork extents and zero fork padding are checked; secondary headers
and Finder comments are skipped using their declared lengths. Packed MacBinary
is unsupported. AppleDouble entry IDs/ranges must be nonoverlapping and unique;
only its resource fork is examined. Its separate data-fork companion is never
opened implicitly: supply that file itself as another input. AppleSingle v2 uses
the same bounded entry machinery but permits both data and resource forks;
the same explicit picture selection rules apply. Entry order is immaterial.
AppleSingle/AppleDouble v1 and automatic native filesystem resource-fork access
are not implemented.

Container scanning, selected-picture parsing and rendering have separate budgets.
The input-byte limit applies to the complete container, not just the selected
picture. Container metadata, unused resource bodies and available padding may
exist outside the selected payload. The original file is preserved.

### Conversion options

| Argument | Default | Behavior |
| --- | --- | --- |
| `--output PATH` | required | Parent must already exist |
| `--format png\|svg\|pdf` | suffix | Explicit format overrides suffix; otherwise `.png` / `.svg` / `.pdf`, case-insensitive |
| `--fidelity strict\|approximate` | `strict` | Strict rejects known approximations; approximate permits only the listed approximations, with warnings |
| `--printer-comments ignore\|apple-portable` | `ignore` | Opt-in documented printer subset below; requires approximate fidelity, never executes PostScript |
| `--vector-policy reject\|preserve\|raster` | `reject` | SVG/PDF requires explicit `preserve` or `raster`; nondefault policies invalid with PNG. Preserve emits supported paths or fails; raster embeds the **complete composited image**, not editable vectors |
| `--width N` | frame width | Positive final pixel width; if height absent, preserve aspect ratio, rounded to nearest integer |
| `--height N` | frame height | Positive final pixel height; if width absent, preserve aspect ratio |
| `--dpi NUMBER` | 72 | Finite, greater than zero and at most 100000; physical metadata only |
| `--background RRGGBB` | `ffffff` | Opaque initial canvas RGB; six hex digits, no `#` prefix |
| `--highlight RRGGBB` | unknown | Explicit system highlight color, used when no HiliteColor is in effect and after DefHilite; highlight use without either fails |
| `--font-map PATH` | none | Schema-1 JSON binding exact source font ID/point size/face to NFNT/FONT resources or explicit portable outline faces; no substitutions |
| `--overwrite` | false | Allow atomic replacement of an unrelated regular output file |

When both dimensions are supplied, output stretches to those dimensions.
When one is supplied and the rounded other dimension is zero, conversion fails.
Raster scaling is deterministic nearest-neighbor **after compositing**, not
reinterpreted pen geometry. Preserved vector paths scale continuously with the
viewport, without first rasterizing. PICT header resolution is available in inspection but does not
silently change output DPI or pixel dimensions. PNG stores rounded pixels per
meter (minimum 1); SVG physical width/height are pixels divided by DPI in inches.
PDF 1.4 uses a single page measured in points (72 per inch), bounded to 14400
points per side. Nonfinite physical dimensions fail. Extremely small DPI may
therefore fail SVG/PDF even though the DPI itself is positive and finite.
Color is device-independent opaque 8-bit RGB in this implementation, not an ICC
conversion of a historical display profile.

`--background` is separate from QuickDraw's own background color/pattern state.
There is no transparency option: destination-dependent Boolean drawing needs a
defined destination.

#### Core CopyBits and PixMaps

Classic indexed depths 1/2/4/8 and direct RGB555/RGB32 use the layouts and
packing inventory in [support.md](support.md#core-pixmapcopybits-operand-inventory).
`pmVersion` 0 and the address-only flag 4 are accepted; serialized addresses are
never followed. Unknown version/color-table flags and nonzero reserved fields
fail instead of being discarded. PICT `packSize` must be zero.

Ordinary CopyBits modes 64-71 are accepted **only** with
`--fidelity approximate`, as base source modes 0-7 with an inactive dither hint.
Neither destination dimension may be smaller than its source rectangle.
Source precision must fit the existing RGB8 profile: monochrome, RGB555/RGB32,
or an explicit indexed palette whose every RGB16 component is exactly an
8-bit value repeated in both bytes. All other flagged reduction cases fail.
This is not an implementation of dithering, error diffusion, palette search
or shrink averaging. Successful use emits `copybits_dither_inactive` with the
operation offset; strict mode rejects it. Existing unflagged modes, tinting,
nearest-neighbor placement, clip/origin/masks and boolean equations are unchanged.
Modes 64/68 can preserve image layers; the other accepted flag combinations
require full-composite vector raster policy. Other flag combinations still fail.
These rules do **not** normalize QuickTime graphics modes or text mask modes.

Source paths (including the font map and fonts), hard-link aliases, symlink outputs, directories and other
nonregular outputs are protected even with `--overwrite`. Output is prepared in
a temporary file in its destination directory, flushed, synced, then atomically
committed. No-clobber commit rejects concurrent creation. Errors before commit
leave an existing output untouched and clean the temporary file. Atomic rename
is not a guarantee against storage/power failure or hostile concurrent parent
directory replacement; use a directory controlled by the invoking user.

### Explicit bitmap fonts

```json
{"schema":1,"fonts":[{"id":7,"size":12,"face":0,"path":"fonts/roman-12.nfnt"}]}
```

Paths are relative to the map's directory unless absolute. Unknown JSON fields,
duplicate bindings and unsupported schema numbers fail. `face` defaults to zero;
it is the recorded QuickDraw style byte. The user asserts that the supplied
resource matches the recorded font identity, size and style; a bare NFNT cannot
independently establish all that identity. FontName records remain inspection
metadata and never trigger system lookup. Include every exact binding used.
Initial recorded font ID/size are zero unless changed by picture opcodes; no
host-specific default font or size is invented.

One-bit NFNT/FONT data can be raw or explicitly selected from a resource container.
No implicit outline conversion, synthetic styling, substitute-size selection,
missing-glyph box or substitution is performed. Single-byte codes address glyphs
directly; no Unicode decoding, Script Manager shaping or multibyte layout is
implemented. Each use reports `explicit_bitmap_font`. Input limits apply to the map and
each resource; collection bytes and binding count are also bounded.

### Explicit portable outline fonts

```json
{"schema":1,"fonts":[{"id":7,"size":12,"face":0,"path":"fonts/roman.ttf","outline":{"face_index":0,"encoding":"macroman","kerning":false}}]}
```

This additive schema-1 binding selects `portable_outline_v1`, **not classic
outline fidelity**. `face_index` and `encoding` are required; `kerning` defaults
to false. A standalone sfnt requires index 0; a TTC requires the exact in-range
face index. The profile accepts static TrueType `glyf` or name-keyed static CFF1
Type 2 outlines with a Unicode cmap and OS/2 style/metrics. CFF2,
variable/color/bitmap font formats and AAT
layout tables are explicitly unsupported. Font resource containers are not
outline containers in this profile: `outline` cannot be combined with
`container`, `resource` or `family`. Existing bitmap bindings keep their contract.
The exact `(id,size,face)` key cannot be duplicated across bitmap and outline
bindings. Size must be positive; only intrinsic regular/bold/italic style bits
are accepted, matching the supplied font's OS/2 flags. Underline, outline,
shadow, condensed/extended and other synthetic face operations are not invented.
No file is discovered by family name, and no font files are shipped for users.

CFF1 uses the same binding, including `.otf` and mixed-outline TTC files. Ordinary
lines/cubic curves/flex, stems/hint masks, and numeric local/global subroutine
calls are supported; rasterization is intentionally unhinted. CFF FontMatrix must
equal the sfnt design-unit scale, with no shear, translation or axis inversion.
CID-keyed CFF, deprecated `seac` components, synthetic/embedded PostScript,
arithmetic/transient-stack operators unsupported by the pinned outline library,
and nonstandard matrices fail explicitly. These are a declared static CFF subset,
not an assertion that every static OpenType font works.
INDEX/DICT extents, operands and masks are checked before contour decoding;
each glyph's expanded program is bounded by the operation/work budgets, with
at most 48 operands, 96 stems and ten subroutine levels. All glyph programs are
preflighted before shaping, including unused glyphs. Contour allocation and
parse/expanded-program work are charged to resource budgets; limits also apply
when rendering with an already loaded font.

`encoding:"macroman"` explicitly asserts Apple's **post-Mac OS 8.5** Roman
mapping, including byte `DB` to U+20AC (euro), `BD` to U+03A9 and `F0` to U+F8FF.
This does not infer the age or script of the PICT. Pre-8.5 currency-sign variants,
other Roman localizations and non-Roman/multibyte scripts need another declared
contract and currently fail if requested. Bytes 00–1F and 7F are rejected.
Missing Unicode cmap entries, glyph zero, unavailable fonts and invalid face
indices fail; no fallback box, decomposed missing-character workaround,
system font or substitute face is supplied.

Each PICT text opcode is shaped independently with locked Rustybuzz/ttf-parser:
Latin script, left-to-right, language `und`, default variation coordinates and
no hinting. Standard required/default OpenType shaping and normalization remain
enabled; optional `liga`, `clig`, `dlig` and `calt` are disabled, and `kern` is
explicitly off/on according to the binding. Thus separate `f` and `i` do not
silently become an optional ligature; a declared MacRoman fi character may map
to its own required glyph. GPOS/GSUB are portable OpenType behavior, not classic
QuickDraw font metrics. Only monotone horizontal clusters are accepted.

Advances and glyph offsets are scaled from font units by exact recorded TxSize
and positive TxRatio into 16.16 fixed-point layout. PnLocHFrac sets initial
horizontal placement. SpExtra applies to byte 20; ChExtra to each other input
byte, once after the last shaped glyph of its cluster. Spacing is scaled
horizontally and never carried as kerning across text opcodes. Delta text
positions still refer to the preceding text start; the graphics pen receives
the floored advanced position. Font ascender/descender and ink/advance extents
bound the whole-run text plane. `FractEnable` does not quantize scalable
outline advances: this profile always retains fractions. `OutlinePreferred`
can be honored by an explicit outline binding; `PreserveGlyph`, nonzero
script-specific line justification and disabled nonunit scaling still fail.

**Nonzero `lineJustify` evidence boundary:** Appendix A footnote 2 identifies
the two Fixed operands as Script Manager extra character width and total extra
width added to a style run. `DrawJustified` describes temporary additions to
SpExtra, but those descriptions do not establish replay's interaction with
already-recorded SpExtra/ChExtra, TxRatio, trailing spaces and state lifetime.
The converter neither redistributes total width nor adds both operands again.
Even declared Roman input returns an explicit unsupported error until an
authoritative replay mapping or provenance-backed recorded-state/output pair
establishes that interaction. Zero records retain existing behavior. Bare bitmap
inputs do not acquire a Roman script by implication.

All outline use requires `--fidelity approximate`, even at native size. Each use
reports `explicit_outline_font` and an approximation diagnostic; conversion JSON
adds `outline_profile:"portable_outline_v1"` when used (otherwise null).
Raster output samples nonzero-winding glyph contours at pixel centers after
bounded curve subdivision (1/64-pixel control deviation, depth at most 16).
Glyph coverage is unioned before whole-run text compositing, so overlaps do not
cancel under XOR. No grid fitting, antialiasing, classic rounding or screen
font strike selection is claimed. Existing text transfer modes, text masks,
origin and clipping apply to that shared run.

Preserved SVG/PDF embeds genuine nonzero-filled glyph paths (quadratics become
equivalent cubics), **not text nodes, font references, pixel-run stand-ins or
images**. Each glyph is painted separately to preserve overlap unions. Copy
modes can paint the bounded background plane first. Supported source-only
outline modes are 0/4, 1 with black foreground, 3 with white background, and 36,
optionally masked. Other modes, including inverse background modes 5/7, require
explicit whole-picture raster fallback. Continuous vector edge coverage can
differ from binary raster sampling at fractional boundaries; editable outline
geometry and classic pixel identity are not interchangeable promises.

Input and aggregate font byte limits apply. Selected sfnt directories, loca
ranges, required tables, composite cycles and expanded glyph complexity are
checked before shaping. Composite depth is at most 16. Shaping has bounded input
and output counts, conservative font-by-run work and scratch-byte accounting;
contours, curve subdivision, pixel-edge tests and serialization consume the
existing budgets. As with image libraries, internal shaper work/allocations are
not an interruptible instruction counter or hard RSS guarantee: use a process
timeout/memory limit for hostile fonts. The font bytes remain explicit protected
inputs and cannot be overwritten by conversion.

#### Resource containers and FOND families (additive schema 1)

```text
quickdraw list-fonts --input fonts.as
```

The result is `{"protocol":1,"inventory":{"container":"applesingle",
"resources":[...]}}`. Entries contain `resource_type` (`NFNT`, `FONT`, `FOND`),
signed `resource_id`, opaque `name_hex` or null, and absolute `offset`/`length`.
Inventory validates all container ranges, not font renderability. It never
extracts files. Resource forks, MacBinary and AppleSingle/AppleDouble v2 work;
raw PICT/file containers are not font containers.

Select an exact resource without family metrics:

```json
{"schema":1,"fonts":[{"id":7,"size":12,"path":"fonts.as","container":"auto","resource":{"type":"NFNT","id":128}}]}
```

Or explicitly select a FOND association at the binding's exact `size`/`face`:

```json
{"schema":1,"fonts":[{"id":7,"size":12,"face":0,"path":"fonts.as","container":"auto","family":{"id":7,"script":"roman","kerning":false}}]}
```

`container` must be present when `resource` or `family` is used; `auto` is
permitted. Without `container`, `path` retains its original raw-NFNT meaning.
A container requires at least one selector. `family.id` is the FOND resource ID
and must agree with its header family ID. The bitmap resource ID comes from
its exact intrinsic monochrome size/style association, not a nearest strike.
Size-zero sfnt associations and depth/style substitutions are unsupported.
If both FONT and NFNT exist at the associated ID, selection is ambiguous:
add `resource` to choose the type explicitly. Its ID must agree with the family
association. Missing/conflicting/ambiguous selections fail, never pick the first.

`family.script` is mandatory and accepts only `roman`; family IDs must be
0..16383. This is an explicit caller assertion, not encoding detection. It does
not translate character bytes or implement Script Manager behavior. Unknown
script strings are invalid font-map arguments (exit 2); unsupported family
semantics fail with exit 5. Bare fonts retain their direct byte-addressed contract.

FOND widths are signed one-point 4.12 values multiplied by the selected point
size, used only for recorded fractional-width layout. NFNT optional 8.8 widths
take precedence; otherwise the exact family style table is used unless ffFlags
bit 14 disables it. With no enabled family table, integer NFNT advances remain
the fallback. No style-property synthesis or missing-glyph replacement occurs.
The missing-character width is parsed but never substituted; the final unused
width word must be zero. Kerning subtables use **pair counts**, as clarified by
Adobe TN 0091 figure 10, not the byte-length description in Apple's prose.

`family.kerning` defaults to false. True explicitly opts into family pair
adjustments **within each text opcode**, before positioning the second glyph.
Amounts are multiplied by point size, then by the horizontal TxRatio, retaining
fixed-point precision. They do not cross positioned runs or depend on the
fractional-width flag. This always requires `--fidelity approximate` and emits
an `approximation` diagnostic describing the policy: PICT does not establish
whether its producer already kerned individual positions. Kerning is never
silently enabled just because the supplied FOND has a table. A requested
kerning style must exist; unspecified pairs have zero adjustment.

Negative family values are supported for version 2 and later (two's complement).
Negative version-0/1 values are rejected when selected: historical descriptions
and readers disagree about one's-complement versus sign-magnitude representation.
Nonnegative values remain usable. Header and optional-directory offsets,
association uniqueness, table extents, width terminators, duplicate style/pair
records, count/work/allocation limits are checked. Printer names/encoding maps,
bounding-box tables and synthetic style properties are bounded opaque metadata,
not applied to the explicitly bound intrinsic bitmap. Their semantic validity
is not certified by this reader.

Complete font container paths and aliases are protected from output replacement,
just like raw font paths. Limits apply to each complete source, container scan,
family parse and aggregate bound fonts; these are not a global process RSS cap.

#### Portable text profile `portable_nfnt_v1`

NFNT optional glyph-width (flag bit 1) and image-height (bit 0) tables are
supported. Each has an entry per encoded glyph plus the missing glyph, without
the location/offset table's extra terminator. Widths are unsigned 8.8 values;
image heights encode top-row offset and row count. Truncation, trailing bytes,
out-of-range heights and tables excluding actual ink fail. Height hints never
silently discard visible pixels or shrink the background transfer rectangle.
Color/deep/synthetic NFNTs remain unsupported.

`glyphState` fractional-widths selects the NFNT 8.8 advance table, then an
explicitly selected enabled FOND table, then integer NFNT advances promoted to
fixed point. No family is inferred. `PreserveGlyph` concerns outline fonts
and has no effect on the supplied bitmap strike. Outline preference and nonzero
`lineJustify` remain explicit errors. Scaling-disabled glyph state with a nonunit
ratio is rejected rather than choosing a substitute strike.

Positive `TxRatio` values scale horizontal and vertical glyph bounds separately
around the baseline, including bearings and advances. Zero denominators are
malformed; zero/negative scales are unsupported. The exact binding still matches
the recorded TxSize, not a guessed larger/smaller font. Native-size drawing needs
no approximation permission; ratio scaling, fractional layout or text modes
other than plain srcOr/srcBic require `--fidelity approximate` and emit an
operation-offset `approximation` diagnostic.

Layout accumulates signed 16.16 advances before taking integer pixel positions.
Recorded `PnLocHFrac` seeds the horizontal accumulator for each positioned run;
its default half-pixel value gives nearest-pixel placement. Negative positions
use mathematical floor, not truncation toward zero. `SpExtra` retains its signed
16.16 fraction and affects spaces only; the PICT `ChExtra` integer operand affects
nonspaces only. Advance plus spacing is scaled horizontally before accumulation.
Glyph bounds are rounded at their edges, with nearest-neighbor sampling within
the resulting integer rectangle. This is a deterministic portable rule, not
a claim about a particular Font Manager/device's rounding. Text deltas remain
relative to the previous text start; the graphics pen receives the advanced
integer position for subsequent line drawing. Scaling and masks are bounded
even when most of a run lies offscreen.

Text source modes 0-7 and arithmetic/transparent modes 32-39 are supported.
The text mask flag 64 restricts the transfer to glyph ink and clips right
overhang at the final advance. Arithmetic text uses the selected foreground and
background RGB values and the existing OpColor requirements; transparency
preserves pixels whose source equals the background. `grayishTextOr` is rejected:
Apple documents that it is not recorded in PICTs.

Glyphs are unioned into one bounded run mask before compositing. Nonmasked modes
apply to both ink and the background/advance cells; mask mode leaves background
pixels untouched. Overlapping glyphs therefore do not XOR each other away within
one run. Separate text operations still composite in order against the complete
destination. Apple's documented stack-dependent splitting can produce different
historical results, hence the explicit portable policy. This does not recreate
Script Manager, automatic family kerning, printer spacing, or classic bitmap scaling.

### Preserved vector policy

`preserve` emits genuine geometry in `portable_paths_v1`: uniform or tiled fills,
rectangular frames and axis-aligned pens, approximate ellipse/rounded-rectangle
paths, filled arc sectors/polygons, region runs, and supported NFNT glyph runs.
Curves/polygons still require `--fidelity approximate`. Region and font paths
preserve pixel-run edges; they are not smooth outline-font reconstruction or
searchable PDF text. Glyph ink is unioned before conversion to nonoverlapping
pixel-run paths.
Bit patterns support patCopy, notPatCopy and transparent; color pixel patterns
copy directly, independent of pen mode. Native SVG/PDF tiling resources retain
the original pattern phase through origin changes.

CopyBits srcCopy, notSrcCopy and transparent placements become embedded images,
with explicit source/destination/clip masks and nearest-neighbor sampling at
native picture resolution. Color notSrcCopy still requires approximate fidelity.
Only the bitmap layer is embedded; preceding/following geometry stays vector.
Binary image masks become PNG alpha/PDF soft masks, never an unqualified fallback
for a destination-dependent operation. SVG viewers may differ in honoring
pixelated image-scaling hints at nonnative display sizes.

Diagonal lines and polygon/arc/complex-region frames now use the raster
interpreter's shared coverage, emitted as nonoverlapping pixel-run rectangles
under `--fidelity approximate`. Rectangular pen stamps are unioned before path
creation; arcs exclude radii and complex regions use the same erosion kernel.
These are editable paths, not generic SVG/PDF strokes or embedded frame images.
Tiled pen patterns, clipping and origin phase remain intact. Coverage and
serialization are bounded; even offscreen line traversal consumes work budget.
Destination-dependent Boolean/arithmetic/highlight modes still fail in this backend.
Text srcCopy/notSrcCopy and transparent use source-only paths, including masks,
scaled glyphs and explicit foreground/background colors. Boolean srcOr/notSrcOr
requires black foreground; srcBic/notSrcBic requires white background.
Other text modes require destination pixels and fail under `preserve`.
Choose `raster` explicitly to use the complete raster
interpreter instead. There is no automatic mixed fallback and no transparent
overlay that loses the preceding destination. Unsupported raster semantics
still fail under the raster policy.

### Explicit portable printer comments

The default is unchanged: `ignore` with strict fidelity rejects comments;
approximate fidelity preserves them in inspection and warns that their semantics
are ignored. It does not apply printer alternatives. This is not a historical
screen-emulation switch.

`--printer-comments apple-portable --fidelity approximate` selects the following
**printer interpretation**, not classic QuickDraw or a particular printer driver.
Conversion JSON adds `printer_comments: "apple_portable"` (or `"ignore"`), and
`portable_printer_comments` identifies the interpretation in diagnostics.
All unsupported comments fail in this profile instead of being silently ignored.

* `TextBegin` (150): exactly 6 bytes, or 10 with an appended 16.16 angle.
  No alignment (`tJus=0`) and zero reserved bytes are required. The appended
  Fixed angle takes precedence when present; the shorter form uses signed
  integer degrees. Positive is clockwise, within -360..360 degrees.
  Horizontal/vertical flips are supported only without nonzero rotation;
  combined flip/rotation order is deliberately outside this profile.
* `TextCenter` (154): exactly two signed Fixed offsets, y then x. Add these
  to the first subsequent text operation's **unrotated baseline** and retain
  that center across subsequent runs. A new TextCenter reanchors at the next
  text operation. TextEnd (151, empty) ends the group. Nested, unmatched,
  unterminated groups and text without TextCenter fail.
* Inside that group, ordinary CopyBits bitmap records are suppressed and
  QuickDraw clipping is ignored, as Appendix B requires for printer text.
  The picture viewport still bounds output. Recorded clip state continues
  to govern drawing after TextEnd. Origin before/after the group works;
  changes inside it are rejected rather than guessing stored-center remapping.
  Other nontext drawing, including embedded QuickTime images, is rejected:
  automatic suspend/resume and re-centering are not implemented.
* Exact supplied NFNT/FONT or portable Roman outline bindings remain required.
  Layout/pen advancement occur before rotation and are not rotated themselves.
  Bitmap ink becomes bounded rectangular contour runs; TrueType/CFF curves
  remain curves. Accepted modes are unmasked black/white srcOr/srcXor/srcBic,
  or transparent text (36). Background-filling/inverse/masked modes and
  printer alignment, line accumulation, font substitution and justification
  are not inferred. Destination-dependent text still requires whole-picture
  raster policy for SVG/PDF.
* `SetLineWidth` (182): exactly two positive signed 16-bit integers, numerator
  then denominator. The profile chooses Appendix B's **multiplicative
  PostScript LaserWriter rule**, not LaserWriter SC's replacement rule.
  Start at 1 per picture; accumulate exact reduced ratios, independently of
  PenSize and text groups. Apply the factor to both current pen dimensions
  at drawing time. Inverse factors restore 1. Bounds are 1/65536..32768;
  rational overflow is a resource error. Zero/negative ratios are unsupported.
  Nonunit factors currently support axis-aligned lines only; shape frames
  and diagonals fail. Lines retain the portable top-left rectangular pen
  footprint, including the endpoint, with fractional extents rather than
  rounded pen sizes. No device-specific stroke alignment is claimed.
* An immediately adjacent, empty PostScriptBegin/PostScriptEnd (190/191)
  pair is accepted as synchronization with no drawing effect in this ordered
  interpreter. Nonempty blocks, executable PostScript comments, string/line
  layout controls and other printer languages are unsupported.

Raster uses native-frame pixel-center coverage before the existing final
nearest-neighbor scaling. Preserved vector paths retain fractional geometry;
thus a subpixel hairline can disappear from native raster yet remain visible
in a high-resolution PDF. This is an explicit presentation difference, not
rounded-away vector widths or a historical printer claim. Prefer preserved
vectors for continuous hairline output. No fallback bitmap is combined with
its printer text alternative.

### Embedded QuickTime still images

Compressed QuickTime opcode `8200` supports `jpeg`, `png `, `raw `, `rle `,
`rpza`, `smc `, `tiff` and `gif `
(spaces are part of the FourCC). JPEG/PNG/TIFF/GIF use established Rust decoders; the
raw/Animation and block-codec readers are bounded implementations of published
formats. Records require version 0, an 86-byte base image description, one frame,
and no separate matte or external palette. An explicitly embedded indexed
raw/Animation/SMC color table and the validated neutral extensions below may extend
that description. Dimensions must match codec headers where
the codec supplies them; block codecs use the description's dimensions.

| Codec | Supported payload |
| --- | --- |
| `jpeg` | 8-bit RGB/grayscale; no CMYK or 16-bit samples |
| `tiff` | One classic TIFF IFD; stripped, chunky, top-left 8-bit gray/RGB/explicit straight RGBA; uncompressed, PackBits, LZW or zlib Deflate |
| `gif ` | One full-screen GIF87a/89a image; explicit global/local palette, interlacing, optional transparent index; no animation/disposal dependency |
| `png ` | Expanded RGB/grayscale, with optional unassociated alpha; no 16-bit PNG or APNG |
| `raw ` | Explicitly paletted depths 1/2/4/8, MSB-first indices; RGB555 (depth 16, high bit zero), RGB24, ARGB32; each scanline padded to even bytes; pad bits/bytes are ignored, not pixels |
| `rle ` | Explicitly paletted depths 1/2/4/8; full-frame RGB555 (unused high bit ignored), RGB24, ARGB32; literal/repeat runs bounded to a row |
| `rpza` | Apple Video 4x4 RGB555 blocks: solid runs, explicit/implicit interpolated quads, and sixteen-color blocks; description depth 16 or 24, no palette |
| `smc ` | Apple Graphics 4x4 indexed blocks: solid/literal, 2/4/8-color caches and same-frame block repeats; depth 8 and a supplied embedded color table |

Animation's big-endian chunk size must match exactly, without unknown size
flags. Header 0 is supported; header 8 is accepted only for start line zero,
all image rows, and zero reserved fields. Every pixel must be supplied by the
still: nonzero skips, early row/frame endings, partial updates and unchanged
frames fail instead of inventing prior-frame pixels. A zero-distance skip is
permitted. At depths 2/4/8, a run unit is four packed bytes containing 16/8/4
MSB-first indices respectively; a negative run repeats that whole unit. A row
must supply exactly `ceil(width / pixels_per_unit)` units; only the last unit's
unused storage pixels are cropped, not looked up in the palette. At depths
16/24/32, a unit remains one pixel. A single final zero terminator is optional
after all rows at these depths; trailing data fails.

Depth 1 has its own command-pair grammar: `(skip, signed run)`; skip bit 7
starts the next row (the first such marker starts row zero), low 7 bits skip
16-pixel groups. Only zero-distance skips are accepted. Positive runs contain
two bytes per group, negative runs below -1 repeat one two-byte group, and -1
is a no-data command, **not** the byte-depth end-of-row marker. Every row must
be complete before starting the next; `(0,0)` terminates the complete still.
Other terminator flags, extra rows and trailing bytes fail. This explicit
interoperability profile is backed by independently decoded fixtures and the
FFmpeg monochrome control description, not a recovered Apple codec oracle.

Raw/Animation description depths **34/36/40** explicitly select 2/4/8-bit
grayscale with the same packed layouts. The profile is a linear white-to-black
ramp: sample zero is white, maximum is black, intermediate output is
`255 - sample * 255 / ((1 << bits) - 1)` in each RGB channel. These bit depths
divide 255 exactly; no color conversion or guessed palette is involved. They
require table ID -1 and cannot be combined with an embedded/resource palette.
Depth 33 (not listed by the cited Apple depth contract), implicit color palettes,
planar raw layouts and temporal frames remain unsupported. Run-command count,
decoded bytes, pixels, padded-group expansion and work are bounded.

RPZA/SMC blocks are consumed left-to-right then top-to-bottom on a grid rounded
up to multiples of four. Edge padding is decoded but cropped from output;
same-frame repetitions retain the entire padded block. Every block must be
defined in this still. Skip commands, early end-of-frame and missing SMC cache
entries fail explicitly, never yielding invented black or prior-frame pixels.
SMC caches reset for each embedded image and wrap at 256 definitions independently
for pairs/quads/octets. Repeating the preceding block or pair is allowed only
after those blocks have been defined in the current image.

The 24-bit chunk length must equal the exact supplied payload length. RPZA
requires marker `0xe1`; SMC accepts the documented `0xe1`/`0x80` markers and
the `0x00` marker emitted by the independently tested FFmpeg encoder. Unknown
markers/opcodes, overruns, truncated operands and trailing data fail. RPZA quad
interpolation floors each weighted 5-bit component before bit replication to
8-bit RGB. Blocks, intermediate buffers, caches, commands and work count against
the existing limits; no native library or decoder subprocess is invoked.

Indexed raw/Animation and SMC require color-table ID 0 and a color table immediately after the image
description's 86-byte base, included in its declared size. The supported QTFF
table has a seed, flags `0x8000`, count-minus-one (1..2^depth entries), and sequential
entries of reserved-zero plus 16-bit RGB components. Components use their high
byte, consistently with existing QuickDraw color handling. Undefined palette
references are malformed. Other flags, nonzero reserved entry values, default
table ID -1, external/resource IDs and description extensions are unsupported;
there is no assumed system palette or inferred transparency.

Description suffixes follow the palette, if any. The supported framing is a
sequence of 32-bit-size/type atoms, each contained entirely in the declared
description, optionally followed by **exactly one final four-byte zero**.
There is no alignment padding between atoms. Truncation, overrun, duplicate
semantic atoms, invalid sizes/rationals or bytes after the terminator fail.
64-bit extended-size framing is checked for bounded extent but remains unsupported.

| Extension | Accepted neutral profile | Rejected rendering-affecting variants |
| --- | --- | --- |
| `fiel` | Exactly two bytes, field count 1 / detail 0 (progressive) | Interlacing, field ordering and other values |
| `pasp` | Exactly two positive unsigned 32-bit spacings with equal ratio (square pixels) | Non-square pixels; zero spacing is malformed |
| `clap` | Exactly eight signed 32-bit rational components; full stored width/height and zero center offsets; equivalent signed ratios normalize exactly | Cropped/offset aperture; zero denominator or nonpositive extent is malformed |
| Other atoms | None silently skipped | `gama`, `colr`, codec configuration and unknown semantic atoms fail; metadata-stream-only definitions such as `btrt` are not assumed valid video extensions |

Neutral hints and terminator bytes do not modify pixels, but are not discarded:
`inspect` bitmap operations add optional
`image_description_extensions: {"offset": absolute_byte_offset, "data_hex": "..."}`
and conversion emits `image_description_extensions`. The bytes are retained
under the decoded-byte budget, and atom count/work are bounded. Gamma, color
management, aperture mapping and pixel-aspect presentation are not inferred
from a successful still decode.

Indexed raw/Animation and RPZA/SMC produce opaque RGB and therefore support recorded srcCopy, not alpha
transfer modes. They use the existing shared affine sampling, source mask,
destination clip/origin and preserved-vector image placement. Existing
They also support recorded image blend mode 32 with explicit OpColor. Direct-color
and JPEG/PNG behavior is unchanged; indexed and block codecs do not supply alpha.

Nonsingular 16.16 affine matrices support rotation, reflection, shear and
fractional edges/translations under `--fidelity approximate`. Projective entries
must be `(0,0,1)` in QuickTime's 2.30 format; singular matrices are malformed.
Transformed source corners give a floor/ceiling destination bounding box.
Each destination pixel center is inverse-mapped with exact integer rational
arithmetic, then floored to its containing source pixel. Samples outside the
source rectangle or source-region mask do not draw; the destination bounding box
is not painted as a rectangle. Current clip and origin apply afterward.
Existing positive axis-aligned placements with integral source edges retain
their original top-left nearest-neighbor sampling, including shrink behavior.
This compatibility distinction is advertised, not a historical filter claim.

| Recorded transfer mode | Portable behavior |
| --- | --- |
| 0, srcCopy | Copy RGB directly, without CopyBits foreground/background tint; nonopaque channels still fail rather than silently discarding alpha |
| 256, straight alpha | Blend source RGB and existing destination by the explicit alpha channel |
| 257, premultiplied white | Remove the recorded white-background contribution, then composite against existing destination |
| 258, premultiplied black | Add the existing destination contribution to the premultiplied source |
| 32, blend | Per-channel source weight from explicit recorded OpColor; opaque images only |
| 36, transparent | Skip a source pixel equal to the explicit OpColor RGB key; copy all other source RGB without tinting |
| 260, straight alpha blend | Per-channel product of explicit OpColor weight and source alpha |

All alpha modes require an explicit channel and `--fidelity approximate`.
Premultiplied modes require ARGB raw/Animation; PNG alpha is unassociated and
is not reinterpreted. Contradictory premultiplied channel ranges are malformed.
Composition uses 8-bit channels and round-to-nearest division by 255, without
ICC/gamma conversion. Mode 256 computes `(source*alpha + dest*(255-alpha)+127)/255`;
mode 258 uses `source*255` for its source term; mode 257 uses
`(source-(255-alpha))*255`. Integer division is applied after summation.
This acts against all preceding drawing, never an empty substitute background.

Modes 32 and 260 require recorded `OpColor` (`001F`) and approximate fidelity,
even if clipping results in no covered pixels. There is no assumed blend level.
Mode 32 reuses the existing QuickDraw component blend:
`floor((source*weight + dest*(65535-weight))/65535)`.
For mode 260, `coverage = alpha*weight` and `denominator = 255*65535`;
output is `(source*coverage + dest*(denominator-coverage) + denominator/2)/denominator`,
integer-rounded once after summation. It does not prematurely reduce OpColor or
the combined opacity to eight bits. Per-channel weights are independent. These
are explicit portable quantization rules, not a claim about historical ICM rounding.
Mode 32 rejects nonopaque alpha rather than discarding it; mode 260 requires
an explicit straight alpha channel.

Mode 36 also requires recorded OpColor and approximate fidelity, even when
clipped away. Compare all three decoded RGB8 components exactly against the
high bytes of OpColor; the low bytes do not introduce tolerances or alpha.
Only an entire matching RGB triple is transparent. Nonmatching pixels copy
unchanged over preceding content, independent of foreground/background colors.
Nonopaque sources fail rather than guessing how alpha combines with keying.
This is the explicit RGB8 profile, not a historical device-depth comparison.
Ordinary CopyBits mode 36 continues to key against QuickDraw background color.
Preserved SVG/PDF represents keyed pixels as transparent pixels on that image
layer, sharing the same source mask, affine sampling, clip and origin rules.

Preserved SVG/PDF shares the exact placement and source-mask sampler. Straight
alpha becomes PNG alpha/PDF soft masks on just the image layer; other geometry
stays vector. Native-size pixels are baked into that layer, not emitted as a
continuously transformed original image. Premultiplied and OpColor blend modes require explicit
whole-picture `--vector-policy raster`, avoiding lossy unpremultiplication or
incorrect transparent overlays. PDF viewers can differ by channel rounding
for partial alpha; that is not a classic QuickDraw fidelity claim.

Separate `matteSize`/`matteRect` payloads still fail explicitly. They are not
equivalent to premultiplied white/black alpha or movie `kmat` atoms. The sources
verified for this milestone do not establish the PICT-specific matte payload
layout, coordinate mapping and interaction with the selected transfer mode;
implementation needs that documentation or a provenance-backed example and
expected result. No sample-independent polarity/layout guess is made.

#### TIFF/GIF single-image and color contract

The existing self-describing-codec image-description depth allowlist remains
8/24/32/40, with color-table ID -1. TIFF/GIF payload headers determine their
actual supported sample layout and palette; no external/default palette is used.

TIFF accepts little- or big-endian classic TIFF, exactly one IFD, strips rather
than tiles, unsigned 8-bit samples, and orientation 1 / chunky storage. Gray
supports declared BlackIsZero or WhiteIsZero; RGBA requires ExtraSamples=2
(unassociated alpha). Horizontal prediction is supported alongside predictor 1.
Strip counts, offsets and lengths are checked before the linked `tiff` decoder.
Compressed strips must decode to exactly their expected size: bounded `weezl`
and `flate2` checks require LZW end codes / complete zlib streams, and PackBits
expansion is checked without allocating its output. Extra decoded samples,
truncated endings and trailing compressed data are errors.
BigTIFF, additional IFDs/pages (including cycles), SubIFDs, tiles, palette TIFF,
CMYK/Lab/YCbCr, associated/unspecified alpha, non-8-bit samples, alternate
orientations and unlisted compression/predictor variants are explicitly unsupported.
In particular, the linked TIFF decoder does not implement palette TIFF; this
converter never interprets its indices as grayscale.

GIF requires exactly one image matching the logical screen at offset zero.
The image's local palette overrides the global palette; absent palettes or
out-of-range pixel/transparency indices fail. A transparent index produces a
straight alpha channel, not a fill using the GIF background index. Interlaced
rows are decoded by the linked `gif` library. Only disposal 0/1, no user-input
flag, and unspecified/square aspect are accepted. Animation, loop/application
extensions, partial-screen frames, restore/background disposal, plain-text
drawing and unknown extensions fail rather than selecting a frame. A single
image's delay and bounded comments do not affect its still pixels.

Both formats reject unsupported color-management semantics instead of silently
applying or discarding a profile: TIFF ICC, transfer functions, chromaticities,
nontrivial color tags and unknown tags are rejected; GIF application extensions
(including color profiles) are rejected. TIFF descriptive ASCII metadata and
valid resolution fields are neutral; placement/resolution comes from the PICT.
No gamma/profile conversion is performed. Samples use the existing portable RGB
contract, not device-color equivalence.

Dimensions must equal the QuickTime description before pixel decoding. Framing,
metadata, strip/block counts, compressed bytes (including aliased TIFF strips),
output buffers and decoder workspace are charged to existing limits. Only one
page/image is admitted, with no new selection option. TIFF/GIF transparency must
use the existing recorded alpha transfer mode; nonopaque srcCopy still fails.
Affine placement, clipping, source masks, OpColor and preserved-image/full-raster
policies are unchanged. No protocol or font-profile change is introduced.

Other transfer modes, projective matrices, rendering-affecting/unknown
image-description extensions, planar raw formats, additional codecs
and opcode `8201` remain unsupported. There is no runtime QuickTime, external
decoder process, network lookup, codec guessing or color-profile conversion.
Decoder-internal work is not a wall-clock guarantee; retain an external timeout
and process-memory limit for untrusted batches.

## JSON and exit codes

Success: exactly one UTF-8 JSON object and newline on stdout; empty stderr.
Failure: empty stdout, exactly one JSON object and newline on stderr.
No progress chatter. A broken output pipe can fail after a file was committed;
the wrapper must consider the process exit status as well as parsing JSON.

```json
{
  "protocol": 1,
  "output": "drawing.png",
  "format": "png",
  "width": 640,
  "height": 480,
  "dpi": 72.0,
  "bytes": 12345,
  "sha256": "<64 lowercase hex characters>",
  "input_sha256": "<64 lowercase hex characters>",
  "raster_profile": "portable_rgb_v1",
  "diagnostics": [
    {
      "code": "unverified_fidelity",
      "message": "Portable RGB raster profile; no classic Mac rendering oracle or historical fonts were used to validate pixel identity."
    }
  ]
}
```

The paths, sizes, hashes and dimensions above illustrate shape, not a fixture.
`output` echoes the supplied path, not necessarily an absolute path.
Hashes refer to the exact bytes read/written, independent of file timestamps.
`input_sha256` covers the whole original container; additive `resource_id` is
the selected PICT ID or null for a data-fork picture.
For preserved vectors `raster_profile` is null and `vector_profile` is
`"portable_paths_v1"`; for raster output `vector_profile` is null.

Diagnostics are structured warnings with `code`, `message`, and an optional
absolute byte `offset`. Codes currently include `unverified_fidelity`,
`approximation`, `reserved_opcode`, `resampled`, `raster_fallback`,
`explicit_bitmap_font`, `vector_presentation`, `embedded_bitmap`,
`embedded_still_image` and
`unsupported_operation` (inspection). A conversion always includes
`unverified_fidelity`; exit 0 is **not** an unqualified fidelity guarantee.
Retain and display warnings rather than discarding them.

```json
{
  "protocol": 1,
  "error": {
    "code": "unsupported",
    "message": "no explicit NFNT font binding for id=7, size=12, face=0",
    "offset": 40,
    "opcode": 40
  }
}
```

`offset` is zero-based in the original input, including any 512-byte header.
`opcode` is an integer (40 here is hexadecimal 0x28). They are omitted for errors
without that context. Messages are descriptive, not stable identifiers.

| Exit | Meaning | Error codes |
| --- | --- | --- |
| 0 | Success (with diagnostics), help, version | no error |
| 2 | Invalid CLI arguments or options | `invalid_arguments` |
| 3 | Filesystem, output conflict or pipe error | `io`, `output_exists`, `output_conflict` |
| 4 | Invalid/truncated/inconsistent PICT | `malformed` |
| 5 | Unsupported layout, operation or policy | `unsupported` |
| 6 | Resource budget exceeded | `resource_limit` |
| 7 | Output encoder failure | `encoding` |

The envelope field `protocol: 1` follows the
[`4d-component-template` Rust starter](https://github.com/miyako/4d-component-template/tree/main/cli-starters/rust).
Its demo `greet` command is intentionally not implemented. A future wrapper should
require protocol 1, check the exit code, retain diagnostics, and treat unknown
additive JSON fields as ignorable. New unsupported inputs must not be mistaken
for success. Incompatible envelope changes require a protocol increment.

### Inspection

`inspect` returns `{"protocol":1,"inspection":{...}}`. The inspection includes:

* `pict_version`, `container`, `source_offset`, `consumed_bytes` (PICT payload only),
  signed `frame` coordinates, optional `resolution_dpi`, and raw `header_hex`.
* Ordered `operations`: original `offset`, numeric `opcode`, semantic `kind`,
  optional `comment_kind`, and `data_hex` for comments, metadata, reserved data and
  known unsupported operations. Drawing pixel arrays are not dumped as JSON.
* `diagnostics` for known unsupported operations. This is **not** an exhaustive
  renderability assessment: modes and approximation policy are checked by render.

An unknown variable-length layout may fail inspection itself. Inspection does
not recover past malformed data or assert that every parsed operation is
renderable. The library keeps structured geometry and decoded bitmaps internally;
inspection is a diagnostic contract, not a complete round-trip PICT serializer.
