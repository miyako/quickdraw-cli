"""Original PixMap rows; PackBits bytes independently encoded/decoded by Pillow/libtiff."""
import io
import json
import struct
from pathlib import Path
from PIL import Image, __version__, features


def words(*values):
    return struct.pack(">" + "H" * len(values), *(v & 65535 for v in values))


def pack(row):
    image = Image.frombytes("L", (len(row), 1), row)
    out = io.BytesIO()
    image.save(out, format="TIFF", compression="packbits")
    with Image.open(io.BytesIO(out.getvalue())) as decoded:
        assert decoded.tobytes() == row
        start = decoded.tag_v2[273][0]
        size = decoded.tag_v2[279][0]
    return out.getvalue()[start:start + size]


def header(width, height, row_bytes, depth, packing, direct, components=1):
    data = struct.pack(">I", 255) if direct else b""
    data += words(0x8000 | row_bytes, 7, -3, 7 + height, width - 3, 4, packing)
    data += struct.pack(">III", 0, 72 << 16, 72 << 16)
    data += words(16 if direct else 0, depth, components, 8 if direct else depth)
    return data + bytes(12)


def rows_packet(rows, row_bytes, packed):
    out = bytearray()
    for row in rows:
        if packed:
            encoded = pack(row)
            out.extend(words(len(encoded)) if row_bytes > 250 else bytes([len(encoded)]))
            out.extend(encoded)
        else:
            out.extend(row)
    return out


def generate():
    cases = []

    def add(name, opcode, data, width, height, rgb):
        cases.append(dict(name=name, opcode=opcode, packet=data.hex(), width=width,
                          height=height, rgb=bytes(rgb).hex()))

    for depth, width, row_bytes, flags in [
        (1,9,8,0), (2,9,8,0x8000), (4,9,8,0), (8,9,10,0x8000),
        (8,249,250,0), (8,251,252,0x8000),
    ]:
        count = min(1 << depth, 4)
        colors = [(i * 73 % 256, (255 - i * 47) % 256, i * 113 % 256) for i in range(count)]
        rows, expected = [], bytearray()
        for y in range(2):
            row = bytearray(row_bytes)
            for x in range(width):
                index = (x // 3 + y) % count
                bit = x * depth
                row[bit // 8] |= index << (8 - depth - bit % 8)
                expected.extend(colors[index])
            rows.append(bytes(row))
        data = header(width, 2, row_bytes, depth, 0, False)
        data += struct.pack(">I", 123) + words(flags, count - 1)
        for i in (range(count) if flags else reversed(range(count))):
            data += words(1234 if flags else i, *(v * 257 for v in colors[i]))
        data += words(7,-3,9,width-3, 0,0,2,width, 0)
        data += rows_packet(rows, row_bytes, True)
        add(f"indexed-{depth}-row{row_bytes}-flags{flags}", 0x98, data, width, 2, expected)

    rgb = bytes([255,0,0, 12,128,241, 0,255,0, 93,41,201])
    for packing, components, width in [(1,3,2),(2,3,2),(4,3,2),(4,4,2),(0,3,2),
                                        (2,3,1),(4,4,1)]:
        row_bytes = width * 4
        expected, rows = bytearray(), []
        for y in range(2):
            row_rgb = rgb[y * 6:y * 6 + width * 3]
            expected.extend(row_rgb)
            if row_bytes < 8 or packing == 1:
                row = b"".join(bytes([77]) + row_rgb[x * 3:x * 3 + 3] for x in range(width))
            elif packing == 2:
                row = row_rgb
            else:
                row = (bytes([77] * width) if components == 4 else b"")
                row += b"".join(row_rgb[c::3] for c in range(3))
            rows.append(row)
        data = header(width,2,row_bytes,32,packing,True,components)
        data += words(7,-3,9,width-3, 0,0,2,width, 0)
        data += rows_packet(rows,row_bytes,row_bytes >= 8 and packing in (0,4))
        add(f"direct32-pack{packing}-components{components}-width{width}",
            0x9a,data,width,2,expected)
    return {
        "provenance": f"Original authored PICT layouts/expected colors, pmVersion=4; row PackBits independently encoded and decoded by Pillow {__version__} / libtiff {features.version('libtiff')}. Not a historical PICT rendering oracle.",
        "cases": cases,
    }


if __name__ == "__main__":
    Path("tests/fixtures/pixmaps.json").write_text(json.dumps(generate(), indent=2) + "\n")
