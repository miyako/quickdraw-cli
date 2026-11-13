"""Original MIT-licensed geometric test fonts; fontTools 4.68.0 regeneration.

These are not historical fonts or a source of QuickDraw fidelity evidence.
No third-party glyph outlines are copied. Native tests use committed files.
"""
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTCollection
from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.pens.t2CharStringPen import T2CharStringPen
from fontTools.misc.psCharStrings import T2CharString
from fontTools.cffLib import SubrsIndex
from fontTools.pens.recordingPen import RecordingPen
import json


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


def cff_font():
    base=font()
    del base["glyf"],base["loca"]
    base.sfntVersion="OTTO"
    base["maxp"].tableVersion=0x5000
    builder=FontBuilder(font=base,isTTF=False)
    builder.setupNameTable({"familyName":"Quickdraw Portable CFF","styleName":"Regular",
                           "uniqueFontIdentifier":"QuickdrawPortableCFF",
                           "fullName":"Quickdraw Portable CFF","psName":"QuickdrawPortableCFF",
                           "licenseDescription":"Original geometric fixture; MIT license, see repository LICENSE."})
    chars={}
    for name in base.getGlyphOrder():
        pen=T2CharStringPen(700,None)
        if name=="O":
            ring(pen)
        elif name=="C":
            pen.moveTo((0,0));pen.lineTo((600,0))
            pen.curveTo((600,900),(0,900),(0,0));pen.closePath()
        elif name!="space":
            rectangle(pen,0,0,600,700)
        chars[name]=pen.getCharString(private=None,globalSubrs=None)
    builder.setupCFF("QuickdrawPortableCFF",{"FullName":"Quickdraw Portable CFF","FamilyName":"Quickdraw Portable CFF","Weight":"Regular"},
                     chars,{"defaultWidthX":700,"nominalWidthX":0})
    top=base["CFF "].cff.topDictIndex[0]
    local=SubrsIndex()
    local.append(T2CharString(program=[600,0,0,700,-600,0,"rlineto","return"],private=top.Private,globalSubrs=top.GlobalSubrs))
    top.Private.Subrs=local
    top.GlobalSubrs.append(T2CharString(program=[-107,"callsubr","return"],private=top.Private,globalSubrs=top.GlobalSubrs))
    # Hints are recorded but not applied by the portable unhinted profile.
    top.CharStrings["A"]=T2CharString(program=[0,20,"hstemhm",0,20,"hintmask",b"\xc0",0,0,"rmoveto",-107,"callgsubr","endchar"],
                                        private=top.Private,globalSubrs=top.GlobalSubrs)
    return base


if __name__ == "__main__":
    root = Path("tests/fixtures")
    regular, wide = font(), font(True)
    regular.save(root / "portable-outline.ttf")
    collection = TTCollection()
    collection.fonts = [regular, wide]
    collection.save(root / "portable-outline.ttc")
    cff=cff_font()
    cff.save(root/"portable-outline-cff.otf")
    collection.fonts=[regular,cff]
    collection.save(root/"portable-outline-mixed.ttc")
    from fontTools.ttLib import TTFont
    loaded=TTFont(root/"portable-outline-cff.otf")
    glyphset=loaded.getGlyphSet()
    paths={}
    for name in loaded.getGlyphOrder():
        pen=RecordingPen();glyphset[name].draw(pen)
        paths[name]=pen.value
    (root/"portable-outline-cff.json").write_text(json.dumps({
        "provenance":"Original geometric fonts generated and independently decompiled by fontTools 4.68.0; not historical glyphs",
        "paths":paths},indent=2)+"\n")
