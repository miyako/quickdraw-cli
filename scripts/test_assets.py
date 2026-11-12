import json
from pathlib import Path
import struct
import tempfile
import unittest
import zipfile

from package import canonical_json, check_binary, create_archive, sha256
from verify_asset import verify


def macho(cpu=0x100000C):
    return b"\xcf\xfa\xed\xfe" + struct.pack("<III", cpu, 0, 2) + bytes(16)


def contents():
    binary = macho()
    manifest = {
        "asset_schema": 1, "protocol": 1, "name": "quickdraw", "version": "0.1.0",
        "target": "aarch64-apple-darwin", "platform": "macos", "architecture": "arm64",
        "binary": "bin/quickdraw", "sha256": sha256(binary), "size": len(binary),
        "signed": False, "notarized": False,
        "files": {"bin/quickdraw": {"sha256": sha256(binary), "size": len(binary)}},
    }
    return {"bin/quickdraw": binary, "manifest.json": canonical_json(manifest)}


class AssetTests(unittest.TestCase):
    def test_archive_is_reproducible_and_permissions_are_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            first, second = Path(directory) / "a.zip", Path(directory) / "b.zip"
            for path in (first, second):
                create_archive(path, contents(), "bin/quickdraw")
            self.assertEqual(first.read_bytes(), second.read_bytes())
            with zipfile.ZipFile(first) as z:
                self.assertEqual(z.getinfo("bin/quickdraw").external_attr >> 16, 0o100755)
            self.assertEqual(verify(first, "aarch64-apple-darwin", sha256(first.read_bytes()))["version"], "0.1.0")

    def test_checksum_target_and_per_file_tampering_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "asset.zip"
            files = contents()
            create_archive(path, files, "bin/quickdraw")
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                verify(path, "aarch64-apple-darwin", "0" * 64)
            with self.assertRaisesRegex(ValueError, "contract mismatch"):
                verify(path, "x86_64-pc-windows-msvc", sha256(path.read_bytes()))
            files["bin/quickdraw"] += b"tampered"
            tampered = Path(directory) / "tampered.zip"
            create_archive(tampered, files, "bin/quickdraw")
            with self.assertRaisesRegex(ValueError, "integrity mismatch"):
                verify(tampered, "aarch64-apple-darwin", sha256(tampered.read_bytes()))

    def test_unsafe_paths_and_unlisted_entries_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            for index, name in enumerate(["../outside", "/absolute", "bin\\outside", "extra", "CON", "bin/name."]):
                files = contents()
                files[name] = b"unexpected"
                path = Path(directory) / f"{index}.zip"
                create_archive(path, files, "bin/quickdraw")
                with self.assertRaises(ValueError):
                    verify(path, "aarch64-apple-darwin", sha256(path.read_bytes()))

    def test_symlink_entries_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "link.zip"
            create_archive(path, contents(), "bin/quickdraw")
            with zipfile.ZipFile(path, "a") as z:
                info = zipfile.ZipInfo("link")
                info.external_attr = 0o120777 << 16
                z.writestr(info, "outside")
            with self.assertRaisesRegex(ValueError, "nonregular"):
                verify(path, "aarch64-apple-darwin", sha256(path.read_bytes()))

    def test_duplicate_entries_fail(self):
        import warnings
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "duplicate.zip"
            create_archive(path, contents(), "bin/quickdraw")
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                with zipfile.ZipFile(path, "a") as z:
                    z.writestr("manifest.json", b"{}")
            with self.assertRaisesRegex(ValueError, "duplicate"):
                verify(path, "aarch64-apple-darwin", sha256(path.read_bytes()))

    def test_malformed_manifest_shapes_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            for index, manifest in enumerate([[], None, True, {"files": []}, {"files": {}, "asset_schema": True, "protocol": 1}]):
                files = contents()
                files["manifest.json"] = canonical_json(manifest)
                path = Path(directory) / f"{index}.zip"
                create_archive(path, files, "bin/quickdraw")
                with self.assertRaises(ValueError):
                    verify(path, "aarch64-apple-darwin", sha256(path.read_bytes()))

    def test_machine_identity_is_read_from_binary(self):
        check_binary(macho(), "aarch64-apple-darwin")
        check_binary(macho(0x1000007), "x86_64-apple-darwin")
        with self.assertRaises(ValueError):
            check_binary(macho(), "x86_64-apple-darwin")
        for invalid in [b"", b"MZ", b"MZ" + bytes(58) + b"\xff" * 4]:
            with self.assertRaises(ValueError):
                check_binary(invalid, "x86_64-pc-windows-msvc")
        pe = bytearray(128)
        pe[:2] = b"MZ"
        struct.pack_into("<I", pe, 60, 64)
        pe[64:68] = b"PE\0\0"
        struct.pack_into("<H", pe, 68, 0x8664)
        struct.pack_into("<H", pe, 86, 2)
        struct.pack_into("<H", pe, 88, 0x20B)
        check_binary(pe, "x86_64-pc-windows-msvc")
        struct.pack_into("<H", pe, 86, 0x2002)
        with self.assertRaises(ValueError):
            check_binary(pe, "x86_64-pc-windows-msvc")


if __name__ == "__main__":
    unittest.main()
