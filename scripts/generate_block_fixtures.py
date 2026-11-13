"""Regenerate authored still fixtures with an independent FFmpeg encoder/decoder.

Not a runtime or native-CI dependency. No historical samples or third-party
implementation source is copied. Run from the repository root with FFmpeg 7.1.1.
"""
import json
from pathlib import Path
import struct
import subprocess
import tempfile


def ffmpeg(*args):
    return subprocess.check_output(["ffmpeg", "-v", "error", *map(str, args)])


def source(codec, pattern, width, height):
    indices = []
    for y in range(height):
        for x in range(width):
            block = (y // 4) * (width // 4) + x // 4
            pos = (y % 4) * 4 + x % 4
            count = {"solid": 1, "pairs": 2, "quads": 4, "octets": 8, "literal": 16}.get(pattern, 1 << (block % 5))
            indices.append((pos % count + (block // 3) * 17) % 256)
    if codec == "smc":
        colors = [(i * 29 % 256, i * 71 % 256, i * 113 % 256) for i in range(256)]
        palette = b"".join(bytes((b, g, r, 255)) for r, g, b in colors)
        return bytes(indices) + palette
    values = [0x001f, 0x2814, 0x500a, 0x7c00]
    if pattern == "quads":
        return b"".join(struct.pack("<H", values[i % 4]) for i in indices)
    return b"".join(struct.pack("<H", ((i * 3 % 32) << 10) | ((i * 7 % 32) << 5) | (i * 11 % 32)) for i in indices)


def generate():
    cases = []
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        for codec in ["rpza", "smc"]:
            for pattern in ["solid", "pairs", "quads", "octets", "literal", "mixed"]:
                width, height = 12, 12
                raw, mov = root / "source.raw", root / "encoded.mov"
                raw.write_bytes(source(codec, pattern, width, height))
                pix_fmt = "pal8" if codec == "smc" else "rgb555le"
                command = ["-f", "rawvideo", "-pixel_format", pix_fmt, "-video_size", f"{width}x{height}",
                           "-i", raw, "-frames:v", "1", "-c:v", codec, "-y", mov]
                ffmpeg(*command)
                encoded = ffmpeg("-i", mov, "-map", "0:v:0", "-c", "copy", "-f", "data", "-")
                rgb = ffmpeg("-i", mov, "-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "-")
                data = mov.read_bytes()
                start = data.index(b"stsd") + 12
                assert data[start + 4:start + 8] == codec.encode().ljust(4)
                depth, table_id = struct.unpack_from(">Hh", data, start + 82)
                table = b""
                if codec == "smc":
                    assert table_id == 0
                    count = struct.unpack_from(">H", data, start + 92)[0] + 1
                    table = data[start + 86:start + 94 + count * 8]
                assert len(rgb) == width * height * 3
                cases.append(dict(name=f"{codec}-{pattern}", codec=codec.ljust(4), width=width, height=height,
                                  depth=depth, palette=table.hex(), packet=encoded.hex(), rgb=rgb.hex()))
        for name, width, height, body in [
            ("rpza-indexed-quad", 4, 4, bytes.fromhex("c07c00001f1b1b1b1b")),
            ("rpza-implicit-quad", 4, 4, bytes.fromhex("7c00801f1b1b1b1b")),
            ("rpza-partial-edges", 5, 3, bytes.fromhex("a17c00")),
        ]:
            encoded = b"\xe1" + (len(body) + 4).to_bytes(3, "big") + body
            packet_file = root / "authored.rpza"
            packet_file.write_bytes(encoded)
            rgb = ffmpeg("-f", "image2", "-video_size", f"{width}x{height}", "-c:v", "rpza",
                         "-i", packet_file, "-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "-")
            assert len(rgb) == width * height * 3
            cases.append(dict(name=name, codec="rpza", width=width, height=height, depth=16,
                              palette="", packet=encoded.hex(), rgb=rgb.hex()))
    output = {
        "provenance": {
            "source": "Original algorithmic color patterns in scripts/generate_block_fixtures.py",
            "encoder_and_reference_decoder": subprocess.check_output(["ffmpeg", "-version"], text=True).splitlines()[0],
            "procedure": "12 authored rawvideo frames -> FFmpeg encoders -> MOV -> packet copy and independent rgb24 decode; 3 additional authored RPZA command cases -> FFmpeg image2 decoder -> rgb24.",
            "scope": "Codec interoperability only; not a historical QuickDraw oracle. Lossy RPZA compares decoded pixels, not original colors.",
        },
        "cases": cases,
    }
    path = Path("tests/fixtures/quicktime-blocks.json")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(output, indent=2) + "\n")
    print(path)


if __name__ == "__main__":
    generate()
