"""Verify an asset without extracting or executing its contents."""

import argparse
import json
from pathlib import Path, PurePosixPath
import re
import sys
import zipfile

from package import TARGETS, check_binary, sha256

MAX_ARCHIVE = 128 * 1024 * 1024


def verify(archive, expected_target, expected_sha256):
    if not re.fullmatch(r"[0-9a-f]{64}", expected_sha256):
        raise ValueError("expected SHA-256 must be 64 lowercase hexadecimal characters")
    if archive.stat().st_size > MAX_ARCHIVE:
        raise ValueError("archive size limit exceeded")
    data = archive.read_bytes()
    if sha256(data) != expected_sha256:
        raise ValueError("archive SHA-256 mismatch")
    with zipfile.ZipFile(archive) as z:
        infos = z.infolist()
        if len(infos) > 2048 or sum(info.file_size for info in infos) > MAX_ARCHIVE:
            raise ValueError("archive expanded size or entry limit exceeded")
        names = [info.filename for info in infos]
        if len(set(names)) != len(names):
            raise ValueError("duplicate archive entry")
        for info in infos:
            path = PurePosixPath(info.filename)
            mode = info.external_attr >> 16
            if path.is_absolute() or ".." in path.parts or "\\" in info.filename or ":" in info.filename:
                raise ValueError("unsafe archive path")
            if not path.parts or any(
                part.endswith((".", " ")) or re.fullmatch(r"(?i)(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?", part)
                for part in path.parts
            ):
                raise ValueError("unsafe Windows archive path")
            if mode & 0o170000 != 0o100000:
                raise ValueError("archive contains a nonregular file")
        manifest = json.loads(z.read("manifest.json"))
        if not isinstance(manifest, dict) or not isinstance(manifest.get("files"), dict):
            raise ValueError("manifest must be an object with a file inventory")
        if type(manifest.get("asset_schema")) is not int or type(manifest.get("protocol")) is not int:
            raise ValueError("manifest schema and protocol must be integers")
        if not isinstance(manifest.get("version"), str) or not re.fullmatch(r"\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?", manifest["version"]):
            raise ValueError("invalid manifest version")
        platform, architecture, filename = TARGETS[expected_target]
        executable = f"bin/{filename}"
        expected = {
            "asset_schema": 1, "protocol": 1, "name": "quickdraw", "target": expected_target,
            "platform": platform, "architecture": architecture, "binary": executable,
            "signed": False, "notarized": False,
        }
        if any(manifest.get(key) != value for key, value in expected.items()):
            raise ValueError("manifest platform, architecture or contract mismatch")
        if set(names) != set(manifest["files"]) | {"manifest.json"}:
            raise ValueError("archive entries differ from manifest inventory")
        for name, entry in manifest["files"].items():
            if not isinstance(entry, dict) or type(entry.get("size")) is not int or not isinstance(entry.get("sha256"), str):
                raise ValueError("invalid file integrity entry")
            content = z.read(name)
            if len(content) != entry["size"] or sha256(content) != entry["sha256"]:
                raise ValueError(f"file integrity mismatch: {name}")
        binary = z.read(executable)
        if len(binary) != manifest["size"] or sha256(binary) != manifest["sha256"]:
            raise ValueError("binary integrity mismatch")
        check_binary(binary, expected_target)
        return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--sha256", required=True, help="Expected digest from a trusted channel")
    args = parser.parse_args()
    try:
        manifest = verify(args.archive, args.target, args.sha256)
        print(json.dumps({"asset_schema": 1, "verified": True, "version": manifest["version"], "target": manifest["target"]}))
    except (ValueError, OSError, KeyError, TypeError, zipfile.BadZipFile) as error:
        print(f"verification failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
