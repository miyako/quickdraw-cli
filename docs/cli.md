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
quickdraw inspect --input PATH [INPUT OPTIONS]
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

### Input options (inspect and convert)

| Argument | Default | Behavior |
| --- | --- | --- |
| `--input PATH` | required | One regular file; never modified |
| `--container auto\|raw\|file` | `auto` | `raw`: PICT starts at byte 0; `file`: skip 512 bytes; `auto`: probe version signatures at both offsets, reject ambiguity |
| `--max-input-bytes N` | 67108864 | Maximum file bytes, checked before and during reading |
| `--max-pixels N` | 16000000 | Maximum frame, bitmap, region, and output pixel count, individually |
| `--max-decoded-bytes N` | 134217728 | Cumulative decoded/scratch allocation accounting per parsing/rendering/vector phase; also bounds aggregate loaded font bytes |
| `--max-operations N` | 100000 | Maximum parsed operations (excluding header and EndPic) |
| `--max-work N` | 100000000 | Parser input/decoding and renderer coverage work budgets, separately |

Every limit must be positive. Increasing them is an explicit resource decision.
Work measures deterministic implementation work, not elapsed seconds. Limits do
not promise a hard process RSS cap: vector capacities, diagnostics, serialized
inspection output, PNG compression and base64 encoding require additional
memory. Use an external process memory limit and timeout for untrusted batches.
Parsing and rendering have separate budgets; a successful parse can still exceed
the rendering budget. Whole-picture rendering precedes output resampling, so a
small requested output does not bypass the native frame limit.

An extracted resource payload is `raw`, not a resource-fork map.
MacBinary, AppleDouble, resource maps, multiple pictures in a file and trailing
data are rejected. No guessed scan for arbitrary embedded signatures is done.
PICT v1 nonzero `picSize` must equal consumed bytes; zero is accepted as unspecified.
PICT v2 `picSize` is ignored, per the format documentation.

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
| `--font-map PATH` | none | Schema-1 JSON binding exact source font ID/point size/face to extracted NFNT/FONT resources; no substitutions |
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

Only extracted one-bit NFNT/FONT data with integer metrics is accepted. No
resource-fork loader, outline font conversion, synthetic styling, rescaling,
missing-glyph box or substitution is performed. Single-byte codes address glyphs
directly; no Unicode decoding, Script Manager shaping or multibyte layout is
implemented. Text supports srcOr/srcBic, native-size glyphs, integer space and
nonspace additions. Scaling ratios, fractional spacing/positioning, nonzero
line justification and outline/fractional-width preferences fail when text uses
them. Each use reports `explicit_bitmap_font`. Input limits apply to the map and
each resource; collection bytes and binding count are also bounded.

### Preserved vector policy

`preserve` emits genuine geometry in `portable_paths_v1`: uniform copy fills,
rectangular frames and axis-aligned pens, approximate ellipse/rounded-rectangle
paths, filled arc sectors/polygons, region runs, and supported NFNT glyph runs.
Curves/polygons still require `--fidelity approximate`. Region and font paths
preserve pixel-run edges; they are not smooth outline-font reconstruction or
searchable PDF text. Overlapping glyph ink uses nonzero fill rather than XOR.

Destination-dependent modes, tiled patterns, bitmap instructions, diagonal
lines and polygon/arc/complex-region framing currently fail in this backend.
Colored Boolean text also fails unless srcOr foreground is black or srcBic
background is white. Choose `raster` explicitly to use the complete raster
interpreter instead. There is no automatic mixed fallback and no transparent
overlay that loses the preceding destination. Unsupported raster semantics
still fail under the raster policy.

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
For preserved vectors `raster_profile` is null and `vector_profile` is
`"portable_paths_v1"`; for raster output `vector_profile` is null.

Diagnostics are structured warnings with `code`, `message`, and an optional
absolute byte `offset`. Codes currently include `unverified_fidelity`,
`approximation`, `reserved_opcode`, `resampled`, `raster_fallback`,
`explicit_bitmap_font`, `vector_presentation` and
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
