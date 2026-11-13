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
| `--max-operations N` | 100000 | Maximum parsed operations (excluding header and EndPic); also bounds container tables/resources and PNG/JPEG segment counts |
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
| `--vector-policy reject\|preserve\|raster` | `reject` | SVG/PDF requires explicit `preserve` or `raster`; nondefault policies invalid with PNG. Preserve emits supported paths or fails; raster embeds the **complete composited image**, not editable vectors |
| `--width N` | frame width | Positive final pixel width; if height absent, preserve aspect ratio, rounded to nearest integer |
| `--height N` | frame height | Positive final pixel height; if width absent, preserve aspect ratio |
| `--dpi NUMBER` | 72 | Finite, greater than zero and at most 100000; physical metadata only |
| `--background RRGGBB` | `ffffff` | Opaque initial canvas RGB; six hex digits, no `#` prefix |
| `--highlight RRGGBB` | unknown | Explicit system highlight color, used when no HiliteColor is in effect and after DefHilite; highlight use without either fails |
| `--font-map PATH` | none | Schema-1 JSON binding exact source font ID/point size/face to raw or container-selected NFNT/FONT resources; no substitutions |
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
No outline font conversion, synthetic styling, substitute-size selection,
missing-glyph box or substitution is performed. Single-byte codes address glyphs
directly; no Unicode decoding, Script Manager shaping or multibyte layout is
implemented. Each use reports `explicit_bitmap_font`. Input limits apply to the map and
each resource; collection bytes and binding count are also bounded.

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

Destination-dependent modes, diagonal lines and polygon/arc/complex-region
framing currently fail in this backend.
Text srcCopy/notSrcCopy and transparent use source-only paths, including masks,
scaled glyphs and explicit foreground/background colors. Boolean srcOr/notSrcOr
requires black foreground; srcBic/notSrcBic requires white background.
Other text modes require destination pixels and fail under `preserve`.
Choose `raster` explicitly to use the complete raster
interpreter instead. There is no automatic mixed fallback and no transparent
overlay that loses the preceding destination. Unsupported raster semantics
still fail under the raster policy.

### Embedded QuickTime still images

Compressed QuickTime opcode `8200` supports `jpeg` and `png ` (the latter FourCC
ends with a space) through established Rust decoders. Supported records use
version 0, an 86-byte image description, one frame, no matte or external palette,
and srcCopy. Positive axis-aligned 16.16 scales/translations are accepted when
the transformed source edges are integral pixels; projective matrix entries use
QuickTime's 2.30 representation and must encode an affine transform. Cropped
source rectangles, source-region transitions, current clip and origin are
preserved. Decoded RGB is copied directly, not tinted with CopyBits foreground
and background colors.

Dimensions must match the codec header and fit pixel/byte budgets. JPEG supports
8-bit RGB/grayscale; PNG supports expanded 8-bit RGB/grayscale and fully opaque
alpha. Truncated payloads, invalid compressed lengths and trailing image data
fail. Nonopaque alpha, mattes, rotation/reflection/perspective, fractional edges,
noncopy QuickTime modes, image-description extensions, CMYK/16-bit JPEG, 16-bit
PNG, animation, other codecs and opcode `8201` remain unsupported. There is no
QuickTime runtime, external decoder process, codec guessing or color-profile
conversion. Decoder-internal work is not a wall-clock guarantee; retain an
external timeout and process-memory limit for untrusted batches.

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
