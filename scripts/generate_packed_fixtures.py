"""Original packed still samples; FFmpeg 7.1.1 independent decode/encode evidence.

Generated packets and reference pixels are committed; native CI needs no FFmpeg.
No historical samples or third-party implementation code is copied.
"""
import json
import struct
import subprocess
import tempfile
from pathlib import Path


def ffmpeg(*args):
    return subprocess.check_output(["ffmpeg", "-v", "error", *map(str, args)])


def atom(tag, data):
    return struct.pack(">I4s", len(data) + 8, tag) + data


def atoms(data):
    at = 0
    while at < len(data):
        size, tag = struct.unpack_from(">I4s", data, at)
        assert size >= 8 and at + size <= len(data)
        yield tag, data[at + 8:at + size]
        at += size


def palette(depth):
    colors = [(i * 71 % 256, (255 - i * 47) % 256, i * 113 % 256) for i in range(1 << depth)]
    table = struct.pack(">IHH", 0, 0x8000, len(colors) - 1)
    table += b"".join(struct.pack(">HHHH", 0, r * 257, g * 257, b * 257) for r, g, b in colors)
    return colors, table


def pack(indices, depth):
    result = bytearray((len(indices) * depth + 7) // 8)
    for i, value in enumerate(indices):
        result[i * depth // 8] |= value << (8 - depth - i * depth % 8)
    return bytes(result)


def movie(template, codec, width, height, depth, table, packet):
    description = bytearray(86)
    struct.pack_into(">I4s", description, 0, 86 + len(table), codec.encode())
    struct.pack_into(">H", description, 14, 1)
    struct.pack_into(">HHII", description, 32, width, height, 72 << 16, 72 << 16)
    struct.pack_into(">IH", description, 44, len(packet), 1)
    struct.pack_into(">HH", description, 82, depth, 0)
    description += table
    def rewrite(data):
        result = b""
        for tag, body in atoms(data):
            if tag in (b"moov", b"trak", b"mdia", b"minf", b"stbl"):
                body = rewrite(body)
            elif tag == b"stsd":
                body = bytes(4) + struct.pack(">I", 1) + description
            elif tag == b"stsz":
                body = bytes(4) + struct.pack(">III", 0, 1, len(packet))
            elif tag == b"stco":
                body = bytes(4) + struct.pack(">II", 1, 8)
            elif tag == b"tkhd":
                body = body[:-8] + struct.pack(">II", width << 16, height << 16)
            result += atom(tag, body)
        return result
    moov = next(body for tag, body in atoms(template) if tag == b"moov")
    return atom(b"mdat", packet) + atom(b"moov", rewrite(moov))


def generate():
    cases = []
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        for depth in (1, 2, 4, 8):
            group = {1: 16, 2: 16, 4: 8, 8: 4}[depth]
            for codec, repeat in [("raw ", False), ("rle ", False), ("rle ", True)]:
                width, height = group * 2 - 1, 2
                colors, table = palette(depth)
                rows = []
                packed_rows = []
                for y in range(height):
                    unit = [(i + y) % (1 << depth) for i in range(group)]
                    row = (unit * 2) if repeat else [(i * 3 + y) % (1 << depth) for i in range(group * 2)]
                    rows.append(row[:width])
                    packed_rows.append(pack(row, depth))
                if codec == "raw ":
                    stride = ((width * depth + 7) // 8 + 1) & ~1
                    packet = b"".join(pack(row, depth).ljust(stride, b"\xa5") for row in rows)
                else:
                    body = b""
                    for row in packed_rows:
                        body += bytes([0x80 if depth == 1 else 1, 254 if repeat else 2])
                        body += row[:group * depth // 8] if repeat else row
                        if depth != 1:
                            body += b"\xff"
                    body += b"\x00\x00" if depth == 1 else b"\x00"
                    packet = struct.pack(">IH", len(body) + 6, 0) + body
                gray = root / "source.gray"
                template_width = (width + 3) & ~3
                gray.write_bytes(bytes((i * 13) % 256 for i in range(template_width * height)))
                template = root / "template.mov"
                ffmpeg("-f", "rawvideo", "-pixel_format", "gray", "-video_size", f"{template_width}x{height}",
                       "-i", gray, "-frames:v", "1", "-c:v", "qtrle", "-y", template)
                sample = root / "sample.mov"
                sample.write_bytes(movie(template.read_bytes(), codec, width, height, depth, table, packet))
                rgb = ffmpeg("-i", sample, "-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "-")
                expected = bytes(channel for row in rows for index in row for channel in colors[index])
                assert rgb == expected, (codec, depth, rgb[:24].hex(), expected[:24].hex())
                cases.append(dict(name=f"{codec.strip()}-{depth}-{'repeat' if repeat else 'literal'}",
                                  codec=codec, width=width, height=height, depth=depth,
                                  palette=table.hex(), packet=packet.hex(), rgb=rgb.hex(),
                                  provenance="Authored packet, independently decoded by FFmpeg"))
        # The available encoder emits 8-bit groups as grayscale depth 40.
        # Keep its packet unchanged and explicitly bind the authored palette at depth 8.
        width, height = 16, 2
        gray = root / "source.gray"
        source = bytes([0, 1, 2, 3] * 4 + list(range(16)))
        gray.write_bytes(source)
        template = root / "encoded.mov"
        ffmpeg("-f", "rawvideo", "-pixel_format", "gray", "-video_size", f"{width}x{height}",
               "-i", gray, "-frames:v", "1", "-c:v", "qtrle", "-y", template)
        packet = ffmpeg("-i", template, "-map", "0:v:0", "-c", "copy", "-f", "data", "-")
        colors, table = palette(8)
        sample = root / "encoded-paletted.mov"
        sample.write_bytes(movie(template.read_bytes(), "rle ", width, height, 8, table, packet))
        rgb = ffmpeg("-i", sample, "-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "-")
        # QuickTime gray stores white at index zero; the encoder inverts the
        # input gray levels. Rebinding a color palette must retain those indices.
        assert rgb == bytes(channel for index in source for channel in colors[255 - index])
        cases.append(dict(name="rle-8-ffmpeg-encoded",codec="rle ",width=width,height=height,depth=8,
                          palette=table.hex(),packet=packet.hex(),rgb=rgb.hex(),
                          provenance="FFmpeg gray encoder packet unchanged (white-zero indices), explicit depth-8 palette, FFmpeg independent decode"))
    output = dict(provenance=dict(source="Original patterns/packets in scripts/generate_packed_fixtures.py",
                                  reference=subprocess.check_output(["ffmpeg","-version"],text=True).splitlines()[0],
                                  scope="Portable packed-layout interoperability, not a historical QuickDraw oracle"),cases=cases)
    path = Path("tests/fixtures/quicktime-packed.json")
    path.write_text(json.dumps(output,indent=2)+"\n")
    print(path)


if __name__ == "__main__":
    generate()
