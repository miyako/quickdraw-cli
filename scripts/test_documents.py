"""Independently parse our generated documents, not a classic QuickDraw oracle."""
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

from pypdf import PdfReader

BINARY = Path(os.environ.get("QUICKDRAW_BINARY", f"target/debug/quickdraw{'.exe' if sys.platform == 'win32' else ''}")).resolve()


def words(*values):
    return struct.pack(">" + "h" * len(values), *values)


def picture(operations):
    data = bytearray(words(0, 0, 0, 10, 20) + b"\x00\x11\x02\xff\x0c\x00")
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

    def convert(self, extension, policy="preserve", operations=None, extra=()):
        source, target = self.root / "input.pict", self.root / f"output.{extension}"
        source.write_bytes(picture(operations or [(0x31, words(0, 0, 10, 20))]))
        result = subprocess.run([str(BINARY), "convert", "--input", str(source), "--output", str(target),
                                 "--vector-policy", policy, "--overwrite", "--dpi", "144", *extra],
                                capture_output=True, check=False)
        return result, target

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


if __name__ == "__main__":
    unittest.main()
