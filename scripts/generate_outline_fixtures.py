"""Original MIT-licensed geometric test fonts; fontTools 4.68.0 regeneration.

These are not historical fonts or a source of QuickDraw fidelity evidence.
No third-party glyph outlines are copied. Native tests use committed files.
"""
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTCollection
from fontTools.feaLib.builder import addOpenTypeFeaturesFromString


def glyph(draw):
    pen = TTGlyphPen(None)
    draw(pen)
    return pen.glyph()


def rectangle(pen, left, bottom, right, top):
    pen.moveTo((left, bottom))
    pen.lineTo((right, bottom))
    pen.lineTo((right, top))
    pen.lineTo((left, top))
    pen.closePath()


def ring(pen):
    rectangle(pen, 0, 0, 600, 700)
    pen.moveTo((150, 150))
    pen.lineTo((150, 550))
    pen.lineTo((450, 550))
    pen.lineTo((450, 150))
    pen.closePath()


def curve(pen):
    pen.moveTo((0, 0))
    pen.lineTo((600, 0))
    pen.qCurveTo((600, 900), (300, 900), (0, 900), (0, 0))
    pen.closePath()


def font(wide=False):
    builder = FontBuilder(1000, isTTF=True)
    names = [".notdef", "space", "A", "V", "O", "C", "eacute", "f", "i", "fi", "trademark", "apple"]
    builder.setupGlyphOrder(names)
    builder.setupCharacterMap({32: "space", 65: "A", 86: "V", 79: "O", 67: "C", 0xe9: "eacute",
                               102: "f", 105: "i", 0xfb01: "fi", 0x2122: "trademark", 0xf8ff: "apple"})
    shapes = {name: glyph(lambda p: rectangle(p, 0, 0, 600 if not wide else 800, 700)) for name in names}
    shapes["space"] = glyph(lambda p: None)
    shapes["O"], shapes["C"] = glyph(ring), glyph(curve)
    composite = TTGlyphPen(shapes)
    composite.addComponent("A", (1, 0, 0, 1, 0, 0))
    shapes["eacute"] = composite.glyph()
    builder.setupGlyf(shapes)
    builder.setupHorizontalMetrics({name: (900 if wide else 700, 0) for name in names})
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    builder.setupNameTable({"familyName": "Quickdraw Portable Test", "styleName": "Regular",
                           "uniqueFontIdentifier": "QuickdrawPortableTestWide" if wide else "QuickdrawPortableTest",
                           "fullName": "Quickdraw Portable Test", "psName": "QuickdrawPortableTest",
                           "licenseDescription": "Original geometric fixture; MIT license, see repository LICENSE."})
    builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=900, usWinDescent=200)
    builder.setupPost()
    builder.setupMaxp()
    addOpenTypeFeaturesFromString(builder.font,
        "languagesystem DFLT dflt; languagesystem latn dflt; "
        "feature kern { pos A V -100; } kern; feature liga { sub f i by fi; } liga;")
    builder.font.recalcTimestamp = False
    builder.font["head"].created = builder.font["head"].modified = 2082844800
    return builder.font


if __name__ == "__main__":
    root = Path("tests/fixtures")
    regular, wide = font(), font(True)
    regular.save(root / "portable-outline.ttf")
    collection = TTCollection()
    collection.fonts = [regular, wide]
    collection.save(root / "portable-outline.ttc")
