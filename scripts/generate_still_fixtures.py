"""Original geometric TIFF/GIF fixtures encoded and decoded with Pillow 11.1.0.

No external image content. The generated JSON is MIT licensed with the repository.
"""
import io
import json
from pathlib import Path
from PIL import Image, TiffImagePlugin, __version__, features


def encode(image, fmt, **kwargs):
    out = io.BytesIO()
    image.save(out, format=fmt, **kwargs)
    return out.getvalue()


def generate():
    rgb = Image.new("RGB", (3, 2))
    rgb.putdata([(255, 0, 0), (0, 255, 0), (0, 0, 255),
                 (33, 66, 99), (255, 255, 255), (0, 0, 0)])
    rgba = rgb.convert("RGBA")
    rgba.putalpha(Image.frombytes("L", (3, 2), bytes([0, 64, 128, 192, 255, 255])))
    gray = Image.frombytes("L", (3, 2), bytes([0, 50, 100, 150, 200, 255]))
    palette = Image.new("P", (3, 2))
    palette.putpalette([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255] + [0] * 756)
    palette.putdata([0, 1, 2, 3, 2, 1])
    cases = []

    def add(name, codec, data, error=None):
        case = {"name": name, "codec": codec, "packet": data.hex()}
        if error:
            case["error"] = error
        else:
            with Image.open(io.BytesIO(data)) as decoded:
                pixels = decoded.convert("RGBA")
                case.update(width=pixels.width, height=pixels.height, rgba=pixels.tobytes().hex())
        cases.append(case)

    for compression in ["raw", "packbits", "tiff_lzw", "tiff_adobe_deflate"]:
        for name, image in [("rgb", rgb), ("rgba", rgba), ("gray", gray)]:
            add(f"tiff-{name}-{compression}", "tiff", encode(image, "TIFF", compression=compression))
    info = TiffImagePlugin.ImageFileDirectory_v2()
    info[262] = 0
    add("tiff-white-zero", "tiff", encode(gray, "TIFF", tiffinfo=info))
    info = TiffImagePlugin.ImageFileDirectory_v2()
    info[317] = 2
    add("tiff-horizontal-predictor", "tiff", encode(rgb, "TIFF", compression="tiff_lzw", tiffinfo=info))
    big = TiffImagePlugin.ImageFileDirectory_v2(prefix=b"MM")
    for tag, value in {256:3,257:2,258:(8,8,8),259:1,262:2,273:(0,),
                       277:3,278:2,279:(18,),284:1}.items():
        big[tag] = value
    out = io.BytesIO()
    big.save(out)
    out.write(rgb.tobytes())
    add("tiff-big-endian", "tiff", out.getvalue())
    add("tiff-multiple-strips", "tiff", encode(rgb, "TIFF", compression="tiff_lzw", strip_size=9))
    add("tiff-pages", "tiff", encode(rgb, "TIFF", save_all=True, append_images=[rgb]), "unsupported")
    add("tiff-palette", "tiff", encode(palette, "TIFF"), "unsupported")
    add("tiff-icc", "tiff", encode(rgb, "TIFF", icc_profile=b"explicit-profile-not-applied"), "unsupported")
    add("gif-global", "gif ", encode(palette, "GIF", optimize=False))
    add("gif-transparent", "gif ", encode(palette, "GIF", optimize=False, transparency=2))
    local = bytearray(encode(palette, "GIF", optimize=False, include_color_table=True))
    local[13:16] = bytes([7, 8, 9])
    add("gif-local", "gif ", bytes(local))
    interlaced = palette.resize((20, 20), Image.Resampling.NEAREST)
    add("gif-interlaced", "gif ", encode(interlaced, "GIF", optimize=False, interlace=True))
    second = palette.copy()
    second.putpixel((0, 0), 1)
    add("gif-frames", "gif ", encode(palette, "GIF", save_all=True, append_images=[second], optimize=False), "unsupported")
    add("gif-loop", "gif ", encode(palette, "GIF", loop=0), "unsupported")
    add("gif-disposal", "gif ", encode(palette, "GIF", disposal=2), "unsupported")
    return {
        "provenance": f"Original geometric images encoded and decoded by Pillow {__version__} / libtiff {features.version('libtiff')}; no historical fidelity claim.",
        "cases": cases,
    }


if __name__ == "__main__":
    Path("tests/fixtures/quicktime-stills.json").write_text(json.dumps(generate(), indent=2) + "\n")
