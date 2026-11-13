"""Render original PICT examples with the CLI, then add explanatory labels.

Requires the existing scripts/requirements-test.txt dependencies and a built CLI.
The geometric font fixtures are original repository artwork, not system fonts.
"""
import argparse
import base64
import hashlib
import html
import io
import json
from pathlib import Path
import struct
import subprocess
import tempfile

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
WIDTH, HEIGHT = 360, 144
TEAL, BLUE, GOLD, RED = (16, 137, 135), (65, 94, 195), (237, 169, 52), (220, 83, 95)


def words(*values):
    return struct.pack(">" + "H" * len(values), *(v & 65535 for v in values))


def rgb(color):
    return words(*(v * 257 for v in color))


def picture(ops):
    result = bytearray(words(0, 0, 0, HEIGHT, WIDTH) + b"\x00\x11\x02\xff\x0c\x00")
    result.extend(b"\xff\xff\xff\xff" + bytes(20))
    for op, payload in [*ops, (255, b"")]:
        if len(result) % 2:
            result.append(0)
        result.extend(words(op) + payload)
    return result


def region(bounds, rows=()):
    result = words(*bounds)
    for y, edges in rows:
        result += words(y, *edges, 32767)
    if rows:
        result += words(32767)
    return words(len(result) + 2) + result


def text(x, y, value):
    encoded = value.encode("ascii")
    return (0x28, words(y, x) + bytes([len(encoded)]) + encoded)


def comment(kind, payload=b""):
    return (0xa1, words(kind, len(payload)) + payload)


def sample_pixmap(name, destination, mode=0):
    fixtures = json.loads((ROOT / "tests/fixtures/pixmaps.json").read_text())["cases"]
    case = next(c for c in fixtures if c["name"] == name)
    packet = bytearray.fromhex(case["packet"])
    end = 50 if case["opcode"] == 0x9a else 54 + 8 * (struct.unpack_from(">H", packet, 52)[0] + 1)
    packet[end + 8:end + 16] = words(*destination)
    packet[end + 16:end + 18] = words(mode)
    return case["opcode"], packet


def qt_image():
    image = Image.new("RGBA", (32, 32), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    draw.rectangle((2, 2, 29, 29), fill=TEAL + (155,))
    draw.ellipse((6, 6, 25, 25), fill=GOLD + (230,))
    stream = io.BytesIO()
    image.save(stream, format="PNG")
    encoded = stream.getvalue()
    matrix = [3*65536, 32768, 0, -65536, 3*65536, 0, 110*65536, 8*65536, 1 << 30]
    description = bytearray(86)
    struct.pack_into(">I4s", description, 0, 86, b"png ")
    struct.pack_into(">HH", description, 32, 32, 32)
    struct.pack_into(">IH", description, 44, len(encoded), 1)
    struct.pack_into(">Hh", description, 82, 32, -1)
    payload = words(0) + struct.pack(">9i", *matrix) + bytes(12) + words(256)
    payload += words(0, 0, 32, 32) + bytes(8) + description + encoded
    return 0x8200, struct.pack(">I", len(payload)) + payload


def examples():
    verbs = [(7, words(5, 5)), (0x1a, rgb(TEAL)), (0x30, words(28, 10, 114, 68)),
             (0x31, words(28, 80, 114, 138)), (0x31, words(28, 150, 114, 208)),
             (0x32, words(46, 161, 97, 197)), (0x31, words(28, 220, 114, 278)),
             (0x33, words(46, 232, 97, 266)), (0x0a, bytes([0xaa, 0x55]*4)),
             (0x34, words(28, 290, 114, 348))]
    shapes = [(0x1a, rgb(TEAL)), (0x0b, words(26, 26)), (0x41, words(18, 12, 78, 106)),
              (0x1a, rgb(BLUE)), (0x51, words(18, 122, 80, 232)),
              (0x1a, rgb(GOLD)), (0x61, words(10, 258, 104, 350, 20, 260))]
    polygon = words(30, 90, 16, 135, 230, 130, 16, 90, 98, 130, 150, 95, 230, 135, 180)
    shapes += [(0x1a, rgb(RED)), (0x71, polygon)]
    lines = [(0x1a, rgb(BLUE)), (7, words(3, 8)), (0x20, words(26, 20, 26, 330)),
             (7, words(7, 3)), (0x20, words(60, 20, 120, 150)),
             (9, bytes([0xaa, 0x55]*4)), (7, words(7, 7)), (0x20, words(65, 195, 118, 330))]
    patterns = []
    for x, color, pattern in [(10, TEAL, [0xaa,0x55]*4), (130, BLUE, [0xff,0xff,0,0]*2),
                              (250, RED, [0x88,0x44,0x22,0x11]*2)]:
        patterns += [(0x1a, rgb(color)), (0x0a, bytes(pattern)), (0x34, words(20,x,124,x+100))]
    clip = region((12, 40, 130, 320), [(12,[40,320]),(44,[110,250]),(98,[110,250]),(130,[40,320])])
    regions = [(0x1a, rgb(TEAL)), (1, clip), (0x31, words(0,0,144,360)),
               (0x1a, rgb(GOLD)), (0x31, words(70,0,86,360))]
    origins = [(0x1a,rgb(TEAL)), (0x31,words(28,210,100,290)),
               (0x0c,words(100,0)), (0x1a,rgb(BLUE)), (0x39,b""),
               (0x0c,words(100,-20)), (0x1a,rgb(GOLD)), (0x39,b"")]
    boolean = [(0x1a,rgb(TEAL)), (0x31,words(25,16,116,144)),
               (0x1a,rgb((0,0,0))), (8,words(10)), (0x31,words(55,84,135,212)),
               (8,words(8)), (0x1a,rgb(BLUE)), (0x31,words(25,250,116,340)),
               (0x1a,rgb((0,0,0))), (8,words(10)), (0x31,words(50,235,135,312)),
               (0x31,words(50,235,135,312))]
    arithmetic = [(0x1a,rgb(BLUE)), (0x31,words(20,12,124,140)),
                  (0x1a,rgb(GOLD)), (0x1f,words(32768,32768,32768)), (8,words(32)),
                  (0x31,words(50,76,136,200)), (0x1d,rgb(TEAL)), (8,words(50)),
                  (0x31,words(30,230,114,344))]
    bitmap = [sample_pixmap("indexed-4-row8-flags0",(20,12,124,180)),
              sample_pixmap("direct32-pack4-components4-width2",(20,204,124,348))]
    bitmap_text = [(3,words(7)), (0x0d,words(3)), (0x10,words(10,10,1,1)),
                   (0x1a,rgb(TEAL)), text(18,54,"ABAB"), (0x16,words(1)),
                   (0x1a,rgb(BLUE)), text(18,118,"ABAB")]
    outlines = [(3,words(8)), (0x0d,words(48)), (0x1a,rgb(TEAL)), text(15,59,"AVOC"),
                (3,words(9)), (0x1a,rgb(BLUE)), text(170,124,"AVOC")]
    stills = [(0x1a,rgb(BLUE)), (0x31,words(35,45,103,250)), qt_image()]
    printer = [(3,words(8)), (0x0d,words(48)), (5,words(1)), (0x1a,rgb((0,0,0))),
               comment(150,words(0,-25,0)), comment(154,bytes(8)), text(34,118,"AVOC"),
               comment(151), (0x1a,rgb(TEAL)), comment(182,words(3,2)),
               (7,words(3,3)), (0x20,words(34,220,34,335)), comment(182,words(2,1)),
               (0x20,words(71,220,71,335)), (0x20,words(108,220,108,335))]
    return [
        ("01", "Shape verbs", "CORE", "Frame / paint / erase / invert / fill", verbs),
        ("02", "Curves and polygons", "PORTABLE", "Rounded corners, ellipses, sectors, polygon", shapes),
        ("03", "Pens and lines", "PORTABLE", "Rectangular nibs and patterned diagonals", lines),
        ("04", "Patterns", "CORE", "Repeating 8x8 fills with separate colors", patterns),
        ("05", "Regions and clipping", "CORE", "A region hole clips every later drawing", regions),
        ("06", "Origin and saved geometry", "CORE", "Same rectangle, changed origin; old ink stays", origins),
        ("07", "Ordered Boolean transfers", "CORE", "XOR overlaps; drawing twice restores ink", boolean),
        ("08", "Arithmetic and highlight", "CORE", "OpColor blend (left); highlight (right)", arithmetic),
        ("09", "Bitmaps and PixMaps", "CORE", "Indexed palette and packed RGB placement", bitmap),
        ("10", "Bitmap font layout", "PORTABLE", "Exact NFNT strike, scaled with extra spacing", bitmap_text),
        ("11", "Portable outline fonts", "PORTABLE", "Supplied TrueType and CFF1 geometric glyphs", outlines),
        ("12", "QuickTime still images", "PORTABLE", "Decoded PNG with alpha and affine placement", stills),
        ("13", "Opt-in printer comments", "PRINTER", "Rotated text and fractional line widths", printer),
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/quickdraw")
    args = parser.parse_args()
    binary = args.binary.resolve()
    output = ROOT / "docs/images"
    output.mkdir(exist_ok=True)
    rendered, records = [], []
    with tempfile.TemporaryDirectory() as tmp:
        directory = Path(tmp)
        nfnt = words(0x1000,65,66,4,0,0,3,3,12,3,0,0,1) + bytes([0x5c,0,0xb4,0,0xfc,0])
        nfnt += words(0,3,5,6,4,0x0103,2,-1)
        (directory / "demo.nfnt").write_bytes(nfnt)
        fonts = [{"id":7,"size":3,"path":"demo.nfnt"}]
        for identifier, filename in [(8,"portable-outline.ttf"),(9,"portable-outline-cff.otf")]:
            fonts.append({"id":identifier,"size":48,"path":str(ROOT / "tests/fixtures" / filename),
                          "outline":{"face_index":0,"encoding":"macroman","kerning":True}})
        (directory / "fonts.json").write_text(json.dumps({"schema":1,"fonts":fonts}))
        for number, title, profile, caption, ops in examples():
            source, target = directory / "demo.pict", directory / "demo.png"
            content = picture(ops)
            source.write_bytes(content)
            command = [str(binary),"convert","--input",str(source),"--output",str(target),"--overwrite"]
            if profile != "CORE":
                command += ["--fidelity","approximate"]
            if number in ("10","11","13"):
                command += ["--font-map",str(directory / "fonts.json")]
            if profile == "PRINTER":
                command += ["--printer-comments","apple-portable"]
            result = subprocess.run(command, capture_output=True, check=False)
            if result.returncode:
                raise RuntimeError(f"{title}: {result.stderr.decode()}")
            report = json.loads(result.stdout)
            png = target.read_bytes()
            with Image.open(io.BytesIO(png)) as image:
                assert image.size == (WIDTH, HEIGHT)
                assert len(image.getcolors(WIDTH * HEIGHT)) > 1, title
                rendered.append((number,title,profile,caption,image.convert("RGB").copy(),png))
            records.append({"id":number,"title":title,"profile":profile,
                            "pict_sha256":hashlib.sha256(content).hexdigest(),
                            "png_sha256":hashlib.sha256(png).hexdigest(),
                            "diagnostics":report["diagnostics"]})

    width, height = 1280, 1790
    image = Image.new("RGB", (width,height), "#101b2a")
    draw = ImageDraw.Draw(image)
    svg = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-labelledby="title desc">',
           '<title id="title">QuickDraw converter: illustrated feature guide</title>',
           '<desc id="desc">Thirteen numbered examples rendered by quickdraw-cli. Core, portable and printer profiles are labeled. Labels are added separately; these are not classic-Mac reference outputs.</desc>',
           f'<rect width="{width}" height="{height}" fill="#101b2a"/>']

    def label(x,y,value,size=18,color="#d7e3ef"):
        draw.text((x,y),value,font=ImageFont.load_default(size=size),fill=color)
        svg.append(f'<text x="{x}" y="{y+size}" fill="{color}" font-family="sans-serif" font-size="{size}">{html.escape(value)}</text>')

    label(38,24,"QUICKDRAW / WHAT THE CONVERTER DOES",34,"#ffffff")
    label(38,76,"Actual PICT conversions. Explicit portable profiles. No classic-fidelity claim.",20)
    label(38,112,"CORE = strict sample   |   PORTABLE = approximate policy   |   PRINTER = separate opt-in",16,"#77dbd3")
    for index,(number,title,profile,caption,frame,png) in enumerate(rendered):
        x, y = 38 + (index % 3)*406, 166 + (index // 3)*292
        draw.rounded_rectangle((x,y,x+390,y+270),radius=12,fill="#1b2b3e")
        svg.append(f'<rect x="{x}" y="{y}" width="390" height="270" rx="12" fill="#1b2b3e"/>')
        label(x+14,y+12,f"{number}  {title}",22,"#ffffff")
        label(x+14,y+44,profile,13,"#77dbd3" if profile=="CORE" else "#ffc875")
        image.paste(frame,(x+15,y+72))
        uri=base64.b64encode(png).decode("ascii")
        svg.append(f'<image x="{x+15}" y="{y+72}" width="{WIDTH}" height="{HEIGHT}" href="data:image/png;base64,{uri}"/>')
        label(x+14,y+233,caption,14)
    label(450,1370,"INGEST  >  INTERPRET  >  EXPORT",26,"#ffffff")
    label(450,1420,"Raw/file PICT, resource forks, MacBinary,",19)
    label(450,1450,"AppleDouble and AppleSingle",19)
    label(450,1494,"PNG pixels / SVG and PDF paths or full raster",19,"#77dbd3")
    label(38,1648,"Images: original synthetic PICTs rendered by this CLI. Captions/layout added separately.",18)
    label(38,1684,"Known unsupported drawing fails; a successful conversion still carries diagnostics.",18)
    label(38,1724,"Read docs/features.md for every supported family, exact boundaries and reproducible examples.",17,"#77dbd3")
    svg.append("</svg>")
    image.save(output / "quickdraw-features.png")
    (output / "quickdraw-features.svg").write_text("\n".join(svg)+"\n")
    (output / "quickdraw-features.json").write_text(json.dumps({"schema":1,"provenance":"Original repository PICT/font artwork; converter PNGs with separately added labels, not a historical oracle.","examples":records},indent=2)+"\n")


if __name__ == "__main__":
    main()
