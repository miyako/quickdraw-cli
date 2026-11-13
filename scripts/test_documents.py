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


def quicktime(codec, width, height, depth, encoded, matrix=None, mode=0, mask=b"", palette=b""):
    matrix = matrix or [65536,0,0, 0,65536,0, 0,0,1<<30]
    payload = words(0) + struct.pack(">9i", *matrix) + bytes(12) + words(mode)
    payload += words(0,0,height,width) + bytes(4) + struct.pack(">I",len(mask)) + mask
    description = bytearray(86)
    struct.pack_into(">I4s", description, 0, 86 + len(palette), codec)
    struct.pack_into(">HH", description, 32, width, height)
    struct.pack_into(">IH", description, 44, len(encoded), 1)
    struct.pack_into(">Hh", description, 82, depth, 0 if palette else -1)
    payload += description + palette + encoded
    return struct.pack(">I",len(payload)) + payload


class Documents(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def test_legacy_block_codecs_match_reference_and_preserve_affine_masked_layers(self):
        cases = json.loads((Path(__file__).resolve().parent.parent / "tests/fixtures/quicktime-blocks.json").read_text())["cases"]
        for case in cases:
            with self.subTest(case=case["name"]):
                width, height = case["width"], case["height"]
                encoded, palette = bytes.fromhex(case["packet"]), bytes.fromhex(case["palette"])
                args = (case["codec"].encode(), width, height, case["depth"], encoded)
                result, target = self.convert("png", operations=[(0x8200, quicktime(*args, palette=palette))],
                                              frame=(0,0,height,width))
                self.assertEqual(result.returncode,0,result.stderr)
                with Image.open(target) as image:
                    self.assertEqual(image.convert("RGB").tobytes(),bytes.fromhex(case["rgb"]))
                if not case["name"].endswith("mixed"):
                    continue
                mask = words(0,0,width//2,32767,height,0,width//2,32767,32767)
                operations = [(0x31,words(0,0,24,24)), (0x0c,words(2,1)),
                              (1,words(10,1,3,23,21)), (0x1a,words(-1,0,-1)),
                              (0x8200,quicktime(*args, palette=palette, mask=mask,
                                  matrix=[-65536,32768,0, 16384,65536,0, 16*65536,32768,1<<30]))]
                self.compare_pdf_with_raster(operations,frame=(0,0,24,24),extra=("--fidelity","approximate"))
                result, target = self.convert("svg",operations=operations,frame=(0,0,24,24),
                                              extra=("--fidelity","approximate"))
                self.assertEqual(result.returncode,0,result.stderr)
                tree = ET.parse(target)
                ns = {"s":"http://www.w3.org/2000/svg"}
                self.assertTrue(tree.findall(".//s:path",ns))
                self.assertEqual(len(tree.findall(".//s:image",ns)),1)

    def convert(self, extension, policy="preserve", operations=None, extra=(), frame=(0,0,10,20)):
        source, target = self.root / "input.pict", self.root / f"output.{extension}"
        source.write_bytes(picture(operations or [(0x31, words(0, 0, 10, 20))], frame))
        result = subprocess.run([str(BINARY), "convert", "--input", str(source), "--output", str(target),
                                 *(["--vector-policy", policy] if extension in ("svg", "pdf") else []),
                                 "--overwrite", "--dpi", "144", *extra],
                                capture_output=True, check=False)
        return result, target

    def compare_pdf_with_raster(self, operations, frame=(0,0,10,20), extra=(), policy="preserve", max_error=0):
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
                expected_bytes = image.convert("RGB").tobytes()
                if max_error:
                    self.assertLessEqual(max(abs(a-b) for a,b in zip(actual.tobytes(),expected_bytes)),max_error)
                else:
                    self.assertEqual(actual.tobytes(),expected_bytes)

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


    def test_affine_raw_animation_png_straight_alpha_layers_match_pdfium(self):
        rgb = bytes([255,0,0, 0,255,0, 0,0,255, 255,255,0])
        argb = bytes([128,255,0,0, 255,0,255,0, 0,0,0,255, 192,255,255,0])
        encoded_png = io.BytesIO()
        rgba = b"".join(argb[i+1:i+4]+argb[i:i+1] for i in range(0,len(argb),4))
        Image.frombytes("RGBA",(2,2),rgba).save(encoded_png,format="PNG")
        rle_body = bytes([1,2])+argb[:8]+bytes([255,1,2])+argb[8:]+bytes([255,0])
        animation = struct.pack(">IH",6+len(rle_body),0)+rle_body
        extra = ("--fidelity","approximate")
        # Clockwise rotation, reflection, shear, fractional translation, shrink.
        for matrix in [
            [0,65536,0, -65536,0,0, 5*65536,2*65536,1<<30],
            [-2*65536,0,0, 0,2*65536,0, 8*65536,65536,1<<30],
            [2*65536,65536,0, 65536,2*65536,0, 2*65536,65536,1<<30],
            [65536,0,0, 0,65536,0, 2*65536+16384,2*65536+49152,1<<30],
            [32768,0,0, 0,32768,0, 2*65536+16384,2*65536+16384,1<<30],
        ]:
            for codec,depth,encoded,mode in [
                (b"raw ",24,rgb,0), (b"raw ",32,argb,256),
                (b"rle ",32,animation,256), (b"png ",32,encoded_png.getvalue(),256),
            ]:
                with self.subTest(matrix=matrix,codec=codec,mode=mode):
                    # The source mask includes x=1 only; it rotates with the source.
                    mask = words(0,1,2,32767, 2,1,2,32767,32767)
                    ops = [(0x1a,words(0x1414,0x2828,0x3c3c)),(0x31,words(0,0,10,20)),
                           (0x0c,words(1,1)),(1,words(10,1,1,9,13)),
                           (0x8200,quicktime(codec,2,2,depth,encoded,matrix,mode)),
                           (0x8200,quicktime(codec,2,2,depth,encoded,matrix,mode,mask))]
                    self.compare_pdf_with_raster(ops,extra=extra,max_error=2 if mode else 0)
                    result, target = self.convert("svg",operations=ops,extra=extra)
                    self.assertEqual(result.returncode,0,result.stderr)
                    self.assertGreater(len(ET.parse(target).findall(".//{http://www.w3.org/2000/svg}image")),0)

    def test_premultiplied_white_black_alpha_requires_complete_raster_fallback(self):
        for mode,encoded in [(257,bytes([128,227,177,152])),(258,bytes([128,100,50,25]))]:
            ops = [(0x1a,words(0x1414,0x2828,0x3c3c)),(0x31,words(0,0,10,20)),
                   (0x8200,quicktime(b"raw ",1,1,32,encoded,mode=mode))]
            extra = ("--fidelity","approximate")
            result,_ = self.convert("pdf",operations=ops,extra=extra)
            self.assertEqual(result.returncode,5)
            self.assertIn(b"premultiplied",result.stderr)
            self.compare_pdf_with_raster(ops,extra=extra,policy="raster")

    def test_quickdraw_frame_paths_match_raster_without_generic_strokes(self):
        polygon = words(26,0,0,7,12, 0,0, 7,12, 0,12, 7,0)
        complex_region = words(44,0,0,8,14, 0,0,14,32767, 2,4,10,32767,
                               6,4,10,32767, 8,0,14,32767,32767)
        shapes = [(0x20,words(-2,-3,8,16)),(0x70,polygon),
                  (0x60,words(0,0,8,14,30,-250)),(0x80,complex_region)]
        extra = ("--fidelity","approximate")
        for shape in shapes:
            for pen in [(1,1),(2,3),(0,2)]:
                for mode,pattern in [(8,bytes([255])*8),(8,bytes([0xaa,0x55])*4),
                                     (12,bytes([0xaa,0x55])*4),(44,bytes([0xaa,0x55])*4)]:
                    with self.subTest(shape=shape[0],pen=pen,mode=mode):
                        ops = [(0x1a,words(0,0,-1)),(0x31,words(-2,-3,10,20)),
                               (0x1a,words(0,0,0)),(0x0c,words(2,1)),(1,words(10,0,0,9,17)),
                               (7,words(*pen)),(8,words(mode)),(9,pattern),shape]
                        self.compare_pdf_with_raster(ops,frame=(-2,-3,10,20),extra=extra)
                        result,target = self.convert("svg",operations=ops,extra=extra,frame=(-2,-3,10,20))
                        self.assertEqual(result.returncode,0,result.stderr)
                        text = target.read_text()
                        tree = ET.parse(target)
                        ns = {"s":"http://www.w3.org/2000/svg"}
                        images = tree.findall(".//s:image",ns)
                        self.assertEqual(images,tree.findall(".//s:pattern/s:image",ns))
                        for image in images:
                            self.assertEqual((image.get("width"),image.get("height")),("8","8"))
                        if pattern == bytes([255])*8:
                            self.assertEqual(images,[])
                        self.assertNotIn("stroke=",text)
        for shape in shapes:
            result,_ = self.convert("pdf",operations=[(8,words(10)),shape],extra=extra)
            self.assertEqual(result.returncode,5)


if __name__ == "__main__":
    unittest.main()
