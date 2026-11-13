"""Deterministic native CLI archives; never publishes or signs a release."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "aarch64-apple-darwin": ("macos", "arm64", "quickdraw"),
    "x86_64-apple-darwin": ("macos", "x86_64", "quickdraw"),
    "x86_64-pc-windows-msvc": ("windows", "x86_64", "quickdraw.exe"),
}


def canonical_json(value):
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True) + "\n").encode()


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def check_binary(data, target):
    """Verify actual PE/Mach-O identity rather than trusting the asset filename."""
    if target.endswith("apple-darwin"):
        if len(data) < 32 or data[:4] != b"\xcf\xfa\xed\xfe":
            raise ValueError("expected a little-endian 64-bit Mach-O executable")
        cpu, _, filetype = struct.unpack_from("<III", data, 4)
        expected = 0x100000C if target.startswith("aarch64") else 0x1000007
        if cpu != expected or filetype != 2:
            raise ValueError("Mach-O architecture or executable file type mismatch")
    else:
        if len(data) < 64 or data[:2] != b"MZ":
            raise ValueError("expected a Windows PE executable")
        offset = struct.unpack_from("<I", data, 60)[0]
        if offset > len(data) - 26 or data[offset:offset + 4] != b"PE\0\0":
            raise ValueError("invalid PE header offset or signature")
        machine = struct.unpack_from("<H", data, offset + 4)[0]
        characteristics = struct.unpack_from("<H", data, offset + 22)[0]
        magic = struct.unpack_from("<H", data, offset + 24)[0]
        if machine != 0x8664 or magic != 0x20B or not characteristics & 2 or characteristics & 0x2000:
            raise ValueError("expected x86_64 PE32+ executable, not a DLL")


def notices(target):
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", target],
        cwd=ROOT,
    ))
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    reached, pending = set(), [metadata["resolve"]["root"]]
    while pending:
        node = pending.pop()
        if node not in reached:
            reached.add(node)
            pending.extend(dep["pkg"] for dep in nodes[node]["deps"])
    files, inventory = {}, []
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["id"] not in reached or package["id"] == metadata["resolve"]["root"]:
            continue
        root = Path(package["manifest_path"]).parent
        selected = []
        for path in sorted(root.rglob("*")):
            if path.is_file() and path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE", "UNLICENSE")):
                relative = path.relative_to(root).as_posix()
                name = f"licenses/{package['name']}-{package['version']}/{relative}"
                files[name] = path.read_bytes()
                selected.append(name)
        if not selected:
            raise ValueError(f"missing redistribution notices for {package['name']} {package['version']}")
        inventory.append({
            "name": package["name"], "version": package["version"],
            "declared_license": package["license"], "notice_files": selected,
        })
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    runtime_notice = sysroot / "share" / "doc" / "rust" / "COPYRIGHT-library.html"
    if not runtime_notice.is_file():
        raise ValueError("Rust COPYRIGHT-library.html is required for runtime redistribution")
    files["licenses/rust/COPYRIGHT-library.html"] = runtime_notice.read_bytes()
    files["licenses/dependencies.json"] = canonical_json({
        "scope": "target-filtered Cargo resolution including build tools; over-inclusive, not a runtime SBOM",
        "packages": inventory,
    })
    return files


def create_archive(path, files, executable):
    with zipfile.ZipFile(path, "x", compression=zipfile.ZIP_STORED) as archive:
        for name, data in sorted(files.items()):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (0o100755 if name == executable else 0o100644) << 16
            info.compress_type = zipfile.ZIP_STORED
            archive.writestr(info, data)


def package(binary, target, output):
    binary = binary.resolve(strict=True)
    rustc = subprocess.check_output(["rustc", "-vV"], text=True)
    host = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
    if host != target:
        raise ValueError(f"native packaging requires host {target}, got {host}")
    if not rustc.startswith("rustc 1.91.0 "):
        raise ValueError("packaging requires the pinned Rust 1.91.0 toolchain")
    if subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=normal"], cwd=ROOT):
        raise ValueError("package from a clean committed checkout")
    source = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    cargo = ROOT / "Cargo.toml"
    import tomllib
    version = tomllib.loads(cargo.read_text())["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?", version):
        raise ValueError("unsupported package version syntax")
    data = binary.read_bytes()
    check_binary(data, target)
    result = subprocess.run([str(binary), "--version"], check=True, capture_output=True)
    if result.stderr or json.loads(result.stdout) != {"protocol": 1, "version": version}:
        raise ValueError("binary protocol/version differs from this checkout")
    platform, architecture, filename = TARGETS[target]
    executable = f"bin/{filename}"
    files = {
        executable: data, "LICENSE": (ROOT / "LICENSE").read_bytes(),
        "README.md": (ROOT / "README.md").read_bytes(),
        "docs/cli.md": (ROOT / "docs/cli.md").read_bytes(),
        "docs/support.md": (ROOT / "docs/support.md").read_bytes(),
        "docs/assets.md": (ROOT / "docs/assets.md").read_bytes(),
        "docs/features.md": (ROOT / "docs/features.md").read_bytes(),
        "docs/images/quickdraw-features.png": (ROOT / "docs/images/quickdraw-features.png").read_bytes(),
        "docs/images/quickdraw-features.svg": (ROOT / "docs/images/quickdraw-features.svg").read_bytes(),
        "docs/images/quickdraw-features.json": (ROOT / "docs/images/quickdraw-features.json").read_bytes(),
        "Cargo.lock": (ROOT / "Cargo.lock").read_bytes(),
    }
    files.update(notices(target))
    manifest = {
        "asset_schema": 1, "protocol": 1, "name": "quickdraw",
        "version": version, "target": target, "platform": platform, "architecture": architecture,
        "binary": executable, "sha256": sha256(data), "size": len(data),
        "source_revision": source, "rustc": rustc.strip(),
        "signed": False, "notarized": False,
        "files": {name: {"sha256": sha256(content), "size": len(content)} for name, content in sorted(files.items())},
    }
    files["manifest.json"] = canonical_json(manifest)
    stem = f"quickdraw-v{version}-{target}"
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"{stem}.zip"
    sidecar = output / f"{stem}.manifest.json"
    checksum = output / f"{stem}.zip.sha256"
    if any(path.exists() for path in (archive, sidecar, checksum)):
        raise ValueError("asset already exists; use a new output directory")
    create_archive(archive, files, executable)
    sidecar.write_bytes(files["manifest.json"])
    checksum.write_text(f"{sha256(archive.read_bytes())}  {archive.name}\n", encoding="ascii", newline="\n")
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    try:
        print(package(args.binary, args.target, args.output))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"packaging failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
