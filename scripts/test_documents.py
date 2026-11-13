"""Independently parse our generated documents, not a classic QuickDraw oracle."""
import json
import io
from contextlib import closing
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

from pypdf import PdfReader
import pypdfium2 as pdfium
from PIL import Image

BINARY = Path(os.environ.get("QUICKDRAW_BINARY", f"target/debug/quickdraw{'.exe' if sys.platform == 'win32' else ''}")).resolve()


def words(*values):
    return struct.pack(">" + "h" * len(values), *values)


def picture(operations, frame=(0, 0, 10, 20)):
    data = bytearray(words(0, *frame) + b"\x00\x11\x02\xff\x0c\x00")
    data.extend(b"\xff\xff\xff\xff" + bytes(20))
    for op, payload in operations:
        if len(data) % 2:
            data.append(0)
        data.extend(struct.pack(">H", op) + payload)
    if len(data) % 2:
        data.append(0)
    data.extend(b"\x00\xff")
    return data


class Documents(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def convert(self, extension, policy="preserve", operations=None, extra=(), frame=(0,0,10,20)):
        source, target = self.root / "input.pict", self.root / f"output.{extension}"
        source.write_bytes(picture(operations or [(0x31, words(0, 0, 10, 20))], frame))
        result = subprocess.run([str(BINARY), "convert", "--input", str(source), "--output", str(target),
                                 "--vector-policy", policy, "--overwrite", "--dpi", "144", *extra],
                                capture_output=True, check=False)
        return result, target

    def compare_pdf_with_raster(self, operations, frame=(0,0,10,20), extra=(), policy="preserve"):
        result, target = self.convert("pdf", policy, operations=operations, frame=frame, extra=extra)
        self.assertEqual(result.returncode, 0, result.stderr)
        expected = self.root / "expected.png"
        result = subprocess.run([str(BINARY),"convert","--input",str(self.root/"input.pict"),
                                 "--output",str(expected),"--overwrite",*extra],capture_output=True)
        self.assertEqual(result.returncode,0,result.stderr)
        with closing(pdfium.PdfDocument(str(target))) as document, \
             closing(document[0]) as page, \
             closing(page.render(scale=2, no_smoothimage=True, no_smoothpath=True)) as bitmap:
            actual = bitmap.to_pil().convert("RGB")
            with Image.open(expected) as image:
                self.assertEqual(actual.size,image.size)
                self.assertEqual(actual.tobytes(),image.convert("RGB").tobytes())

    def test_pdf_xref_page_content_and_physical_transform(self):
        result, target = self.convert("pdf")
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["vector_profile"], "portable_paths_v1")
        self.assertIsNone(report["raster_profile"])
        reader = PdfReader(target, strict=True)
        self.assertEqual(len(reader.pages), 1)
        page = reader.pages[0]
        self.assertEqual(list(page.mediabox), [0, 0, 10, 5])
        self.assertNotIn("/XObject", page["/Resources"])
        ops = page.get_contents().operations
        self.assertIn(b"f*", [op for _, op in ops])
        self.assertIn(([0.5, 0, 0, -0.5, 0, 5], b"cm"), ops)
        self.assertFalse(reader.is_encrypted)

    def test_pdf_whole_composite_fallback_keeps_xor_destination(self):
        ops = [(0x31, words(0,0,10,20)), (0x33, words(0,0,10,10))]
        result, target = self.convert("pdf", "raster", ops)
        self.assertEqual(result.returncode, 0, result.stderr)
        page = PdfReader(target, strict=True).pages[0]
        image = page["/Resources"]["/XObject"]["/Im0"]
        self.assertEqual((image["/Width"],image["/Height"]),(20,10))
        self.assertEqual(image.get_data(), (bytes([255])*30 + bytes(30))*10)
        self.assertIn("raster_fallback", [d["code"] for d in json.loads(result.stdout)["diagnostics"]])

    def test_svg_clip_origin_and_paths_are_well_formed(self):
        operations = [(1,words(10,0,1,10,19)), (0x0c,words(2,1)), (0x31,words(1,2,5,8))]
        result, target = self.convert("svg", operations=operations)
        self.assertEqual(result.returncode,0,result.stderr)
        tree = ET.parse(target)
        ns = {"s":"http://www.w3.org/2000/svg"}
        self.assertEqual(len(tree.findall(".//s:image",ns)),0)
        self.assertEqual(len(tree.findall(".//s:clipPath",ns)),1)
        self.assertIn("translate(-2 -1)",[g.get("transform") for g in tree.findall(".//s:g",ns)])
        self.assertEqual(tree.getroot().get("viewBox"),"0 0 20 10")

    def test_vector_failure_does_not_replace_existing_document(self):
        target = self.root / "output.pdf"
        target.write_bytes(b"existing")
        result, _ = self.convert("pdf", operations=[(8,words(10)),(0x31,words(0,0,10,20))])
        self.assertEqual(result.returncode,5)
        self.assertEqual(target.read_bytes(),b"existing")
        self.assertEqual(json.loads(result.stderr)["error"]["opcode"],0x31)

    def test_explicit_bitmap_glyph_runs_use_nonzero_fill_for_overlapping_ink(self):
        font = words(0x1000,65,66,4,0,0,3,3,12,3,0,0,1)
        font += bytes([0x5c,0,0xb4,0,0xfc,0]) + words(0,3,5,6) + words(4,0x0103,2,-1)
        (self.root / "font.nfnt").write_bytes(font)
        mapping = self.root / "fonts.json"
        mapping.write_text(json.dumps({"schema":1,"fonts":[{"id":7,"size":3,"path":"font.nfnt"}]}))
        ops = [(3,words(7)),(0x0d,words(3)),(0x16,words(-4)),
               (0x28,words(3,0)+bytes([2])+b"AA")]
        result, target = self.convert("pdf",operations=ops,extra=("--font-map",str(mapping)))
        self.assertEqual(result.returncode,0,result.stderr)
        operations = PdfReader(target,strict=True).pages[0].get_contents().operations
        self.assertIn(b"f",[op for _,op in operations])
        self.assertNotIn(b"f*",[op for _,op in operations])
        self.assertNotIn(b"Do",[op for _,op in operations])

    def test_masked_scaled_bitmap_placement_matches_raster_in_pdfium(self):
        header = words(2,5,7,7,9, 5,7,7,9, 2,3,8,11, 0)
        mask = words(10,3,5,7,10)
        ops = [(0x31,words(0,0,10,20)),(0x1a,words(-1,0,0)),(0x1b,words(0,0,-1)),
               (0x0c,words(1,1)),(1,words(10,1,0,9,18)),
               (0x91,header+mask+bytes([0x80,0,0x40,0]))]
        self.compare_pdf_with_raster(ops)
        result, target = self.convert("svg",operations=ops)
        self.assertEqual(result.returncode,0,result.stderr)
        tree = ET.parse(target)
        images = tree.findall(".//{http://www.w3.org/2000/svg}image")
        self.assertEqual(len(images),1)
        self.assertTrue(images[0].get("{http://www.w3.org/1999/xlink}href").startswith("data:image/png;base64,"))

    def test_tiled_copy_inversion_and_transparency_keep_pattern_phase(self):
        for mode in (8,12,44):
            for frame in ((0,0,10,20),(-2,-3,8,17)):
                ops = [(0x1a,words(0,0,-1)),(0x31,words(*frame)),
                       (0x1a,words(0,-1,0)),(9,bytes([128,64,32,16,8,4,2,1])),
                       (8,words(mode)),(0x0c,words(3,1)),(0x31,words(1,3,9,19))]
                self.compare_pdf_with_raster(ops,frame)

    def test_quicktime_png_source_mask_and_transform_preserve_backdrop(self):
        image = Image.new("RGB",(2,2))
        image.putdata([(255,0,0),(0,255,0),(0,0,255),(255,255,0)])
        buffer = io.BytesIO()
        image.save(buffer,format="PNG")
        encoded = buffer.getvalue()
        mask = words(0,0,1,32767, 2,0,1,32767, 32767)
        payload = words(0)+struct.pack(">9i",131072,0,0,0,131072,0,196608,131072,1<<30)
        payload += bytes(14)+words(0,0,2,2)+bytes(4)+struct.pack(">I",len(mask))+mask
        desc = bytearray(86)
        struct.pack_into(">I4s",desc,0,86,b"png ")
        struct.pack_into(">HH",desc,32,2,2)
        struct.pack_into(">IH",desc,44,len(encoded),1)
        struct.pack_into(">Hh",desc,82,24,-1)
        payload += desc+encoded
        ops = [(0x31,words(0,0,10,20)),(0x0c,words(1,1)),
               (0x8200,struct.pack(">I",len(payload))+payload)]
        self.compare_pdf_with_raster(ops)

    def test_fractional_scaled_nfnt_text_and_source_modes_match_pdfium_pixels(self):
        font = words(0x1003,65,66,4,0,0,3,3,12,3,0,0,1)
        font += bytes([0x5c,0,0xb4,0,0xfc,0]) + words(0,3,5,6)
        font += words(4,0x0103,2,-1, 0x0280,0x0380,0x0200, 3,3,3)
        (self.root / "font.nfnt").write_bytes(font)
        mapping = self.root / "fonts.json"
        mapping.write_text(json.dumps({"schema":1,"fonts":[{"id":7,"size":3,"path":"font.nfnt"}]}))
        extra = ("--font-map",str(mapping),"--fidelity","approximate")
        for mode in (0,1,3,4,5,7,36,64,68):
            with self.subTest(mode=mode):
                ops = [(0x1a,words(0,0,-1)),(0x31,words(-2,-3,8,17)),
                       (0x1a,words(0,0,0)),(3,words(7)),(0x0d,words(3)),
                       (0x2e,words(4)+bytes([0,1,1,0])),(0x10,words(2,3,1,2)),
                       (0x15,words(0x4000)),(0x0c,words(2,1)),
                       (1,words(10,0,1,9,15)),(5,words(mode)),
                       (0x28,words(6,0)+bytes([3])+b"ABA"),
                       (0x21,words(6,17))]
                self.compare_pdf_with_raster(ops,(-2,-3,8,17),extra)
                result, target = self.convert("svg",operations=ops,extra=extra,frame=(-2,-3,8,17))
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertEqual(len(ET.parse(target).findall(".//{http://www.w3.org/2000/svg}image")),0)
        ops = [(0x31,words(0,0,10,20)),(3,words(7)),(0x0d,words(3)),
               (5,words(2)),(0x28,words(3,0)+bytes([2])+b"AB")]
        self.compare_pdf_with_raster(ops,extra=extra,policy="raster")

    def test_applesingle_fond_fractional_widths_and_opt_in_kerning_match_pdfium(self):
        font = words(0x1000,65,66,4,0,0,3,3,12,3,0,0,1)
        font += bytes([0x5c,0,0xb4,0,0xfc,0]) + words(0,3,5,6) + words(4,0x0103,2,-1)
        family = words(2,7,65,66,4096,0,0,4096) + struct.pack(">III",60,72,0)
        family += bytes(22) + words(3,0,3,0,-128)
        family += words(0,0,3072,4096,2048,0, 0,0,1,0x4142,-1024)
        payload = struct.pack(">I",len(family)) + family + struct.pack(">I",len(font)) + font
        resource_map = bytearray(70)
        resource_map[24:30] = words(28,70,1)
        resource_map[30:46] = b"FOND" + words(0,18) + b"NFNT" + words(0,30)
        for at, resource_id, offset in [(46,7,0),(58,-128,4+len(family))]:
            resource_map[at:at+4] = words(resource_id,-1)
            resource_map[at+5:at+8] = offset.to_bytes(3,"big")
        header = struct.pack(">IIII",256,256+len(payload),len(payload),len(resource_map))
        resource_map[:16] = header
        fork = header + bytes(240) + payload + resource_map
        single = struct.pack(">II",0x00051600,0x00020000) + bytes(16) + words(1)
        single += struct.pack(">III",2,38,len(fork)) + fork
        (self.root/"fonts.as").write_bytes(single)
        mapping = self.root/"fonts.json"
        extra = ("--font-map",str(mapping),"--fidelity","approximate")
        for kerning in (False, True):
            mapping.write_text(json.dumps({"schema":1,"fonts":[{
                "id":7,"size":3,"path":"fonts.as","container":"auto",
                "family":{"id":7,"script":"roman","kerning":kerning}}]}))
            for mode in (0,64):
                with self.subTest(kerning=kerning,mode=mode):
                    ops = [(3,words(7)),(0x0d,words(3)),(0x2e,words(4)+bytes([0,0,1,0])),
                           (0x10,words(1,2,1,1)),(0x0c,words(2,1)),(1,words(10,0,1,8,15)),
                           (5,words(mode)),(0x28,words(4,1)+bytes([3])+b"ABA"),(0x21,words(4,15))]
                    self.compare_pdf_with_raster(ops,extra=extra)
                    result, target = self.convert("svg",operations=ops,extra=extra)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertEqual(len(ET.parse(target).findall(".//{http://www.w3.org/2000/svg}image")),0)


if __name__ == "__main__":
    unittest.main()
