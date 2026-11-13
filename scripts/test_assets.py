import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
import zipfile

from package import canonical_json, check_binary, create_archive, macos_release_evidence, sha256
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


def universal(wide=False):
    data = bytearray(192)
    struct.pack_into(">II", data, 0, 0xcafebabf if wide else 0xcafebabe, 2)
    for index, (cpu, offset) in enumerate([(0x100000C, 128), (0x1000007, 160)]):
        struct.pack_into(">IIQQII" if wide else ">IIIII", data, 8+index*(32 if wide else 20),
                         cpu, 0, offset, 32, 4, *([0] if wide else []))
        data[offset:offset+32] = macho(cpu)
    return data


class AssetTests(unittest.TestCase):
    def test_universal_architecture_table_is_checked_against_bounded_disjoint_slices(self):
        for wide in (False, True):
            good=universal(wide)
            check_binary(good,"universal-apple-darwin")
            for cut in (0,7,16,47,127,191):
                with self.assertRaises(ValueError):
                    check_binary(good[:cut],"universal-apple-darwin")
        for offset, value in [(4,1),(28,0x100000C),(36,128),(40,1000),(44,31),(164,0x100000C)]:
            bad=universal()
            struct.pack_into(">I",bad,offset,value)
            with self.assertRaises(ValueError):
                check_binary(bad,"universal-apple-darwin")
        with self.assertRaises(ValueError):
            check_binary(macho(),"universal-apple-darwin")

    def test_universal_release_requires_consistent_signing_and_notary_metadata(self):
        # Synthetic headers check manifest structure, not cryptographic authenticity.
        binary=universal()
        files=contents()
        manifest=json.loads(files["manifest.json"])
        manifest.update(target="universal-apple-darwin",architecture="universal",
                        architectures=["arm64","x86_64"],signed=True,notarized=True,
                        size=len(binary),sha256=sha256(binary),
                        signing={"identity_type":"Developer ID Application","team_id":"0123456789",
                                 "hardened_runtime":True,"secure_timestamp":True},
                        notarization={"status":"Accepted","id":"12345678-1234-1234-1234-123456789abc",
                                      "submission_sha256":"a"*64,"binary_sha256":sha256(binary),"stapled":False})
        manifest["files"]["bin/quickdraw"]={"size":len(binary),"sha256":sha256(binary)}
        files["bin/quickdraw"]=binary
        with tempfile.TemporaryDirectory() as directory:
            for index, change in enumerate([{}, {"signed":False}, {"notarized":1},
                                            {"architectures":["arm64"]},
                                            {"notarization":{**manifest["notarization"],"binary_sha256":"b"*64}}]):
                files["manifest.json"]=canonical_json({**manifest,**change})
                path=Path(directory)/f"{index}.zip"
                create_archive(path,files,"bin/quickdraw")
                if change:
                    with self.assertRaises(ValueError):
                        verify(path,"universal-apple-darwin",sha256(path.read_bytes()))
                else:
                    verify(path,"universal-apple-darwin",sha256(path.read_bytes()))

    def test_notary_evidence_must_contain_exact_final_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            binary,receipt,submission=root/"quickdraw",root/"receipt.json",root/"submission.zip"
            binary.write_bytes(universal())
            receipt.write_text(json.dumps({"status":"Accepted","id":"12345678-1234-1234-1234-123456789abc"}))
            with zipfile.ZipFile(submission,"w") as archive:
                archive.writestr("quickdraw",b"different bytes")
            with self.assertRaisesRegex(ValueError,"exact signed binary"):
                macos_release_evidence(binary,receipt,submission)
            receipt.write_text('{"status":"Invalid"}')
            with self.assertRaisesRegex(ValueError,"not accepted"):
                macos_release_evidence(binary,receipt,submission)

    def test_release_signature_policy_checks_each_architecture_and_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            binary,receipt,submission=root/"quickdraw",root/"receipt.json",root/"submission.zip"
            binary.write_bytes(universal())
            receipt.write_text(json.dumps({"status":"Accepted","id":"12345678-1234-1234-1234-123456789abc"}))
            with zipfile.ZipFile(submission,"w") as archive:
                archive.writestr("quickdraw",binary.read_bytes())
            good="CodeDirectory flags=0x10000(runtime)\nAuthority=Developer ID Application: Test\nTeamIdentifier=0123456789\nTimestamp=example\n"
            for detail in [good,good.replace("runtime","adhoc"),good.replace("Timestamp=","NoTimestamp="),
                           good.replace("Developer ID Application:","Signature=adhoc"),
                           good.replace("0123456789","9876543210")]:
                def run(command, **kwargs):
                    # A bad second slice must not be hidden by a valid first slice.
                    return SimpleNamespace(stderr=detail if "x86_64" in command else good)
                with patch("package.subprocess.run",side_effect=run) as calls, patch.dict("os.environ",QUICKDRAW_SIGNING_TEAM_ID="0123456789"):
                    if detail==good:
                        result=macos_release_evidence(binary,receipt,submission)
                        self.assertTrue(result["signed"] and result["notarized"])
                        self.assertEqual(sum("--architecture" in call.args[0] for call in calls.call_args_list),2)
                    else:
                        with self.assertRaises(ValueError):
                            macos_release_evidence(binary,receipt,submission)

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
