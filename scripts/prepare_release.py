"""Validate tested native archives and assemble the two release deliverables."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import zipfile

from package import check_binary, sha256
from verify_asset import verify


def prepare(inputs, work, final, revision, version):
    work.mkdir(parents=True, exist_ok=False)
    final.mkdir(parents=True, exist_ok=False)
    slices = []
    for target in ("aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc"):
        stem = f"quickdraw-v{version}-{target}"
        directory = inputs / f"quickdraw-{target}"
        archive = directory / (stem + ".zip")
        digest = directory.joinpath(stem + ".zip.sha256").read_text().split()[0]
        manifest = verify(archive, target, digest)
        if manifest.get("source_revision") != revision or manifest["version"] != version:
            raise ValueError("native build revision/version does not match release")
        if target.endswith("apple-darwin"):
            with zipfile.ZipFile(archive) as z:
                path = work / target
                path.write_bytes(z.read("bin/quickdraw"))
                slices.append(path)
        else:
            for suffix in (".zip", ".zip.sha256", ".manifest.json"):
                source = directory / (stem + suffix)
                if suffix == ".manifest.json" and json.loads(source.read_text()) != manifest:
                    raise ValueError("manifest sidecar mismatch")
                shutil.copyfile(source, final / source.name)
    binary = work / "quickdraw"
    subprocess.run(["lipo", "-create", *map(str, slices), "-output", str(binary)], check=True)
    binary.chmod(0o755)
    check_binary(binary.read_bytes(), "universal-apple-darwin")
    print(json.dumps({"revision": revision, "version": version, "unsigned_universal_sha256": sha256(binary.read_bytes())}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inputs", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--final", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    prepare(args.inputs, args.work, args.final, args.revision, args.version)
