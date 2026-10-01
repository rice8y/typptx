#!/usr/bin/env python3
"""Validate release versions and create portable binary ZIPs (Python 3.11+)."""

import argparse
import hashlib
from pathlib import Path
import re
import stat
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PLATFORMS = ("macos-arm64", "macos-x86_64", "linux-x86_64", "windows-x86_64")


def release_version(root: Path, tag: str) -> str:
    manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    version = manifest["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError(f"unsupported release version: {version!r}")
    if tag != f"v{version}":
        raise ValueError(f"tag {tag!r} does not match Cargo.toml (expected v{version})")
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    versions = [p["version"] for p in lock["package"] if p["name"] == "typptx" and "source" not in p]
    if versions != [version]:
        raise ValueError("Cargo.lock does not match the Typptx package version")
    return version


def package_binary(root: Path, binary: Path, platform: str, version: str, output: Path) -> Path:
    executable = "typptx.exe" if platform.startswith("windows-") else "typptx"
    files = {
        executable: binary,
        "README.md": root / "README.md",
        "LICENSE": root / "LICENSE",
        "assets/logo.svg": root / "assets/logo.svg",
        "licenses/hb-subset-MIT.md": root / "vendor/hb-subset/LICENSE.md",
        "licenses/HarfBuzz-COPYING": root / "vendor/hb-subset/harfbuzz/COPYING",
    }
    for path in files.values():
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"missing or empty release file: {path}")
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"typptx-v{version}-{platform}.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as bundle:
        for name, path in files.items():
            entry = zipfile.ZipInfo(name)
            entry.create_system = 3  # Preserve Unix executable permissions even on Windows.
            mode = 0o755 if name == executable else 0o644
            entry.external_attr = (stat.S_IFREG | mode) << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            bundle.writestr(entry, path.read_bytes())
    return archive


def write_checksums(output: Path, version: str) -> Path:
    expected = {f"typptx-v{version}.zip"}
    expected.update(f"typptx-v{version}-{platform}.zip" for platform in PLATFORMS)
    actual = {path.name for path in output.glob("*.zip")}
    if actual != expected:
        raise ValueError(f"release archives differ: missing={sorted(expected - actual)}, unexpected={sorted(actual - expected)}")
    lines = []
    for name in sorted(expected):
        with (output / name).open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        lines.append(f"{digest}  {name}\n")
    checksums = output / "SHA256SUMS"
    checksums.write_text("".join(lines), encoding="utf-8", newline="\n")
    return checksums


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    check = commands.add_parser("check")
    check.add_argument("tag")
    package = commands.add_parser("package")
    package.add_argument("tag")
    package.add_argument("--platform", choices=PLATFORMS, required=True)
    package.add_argument("--binary", type=Path, required=True)
    package.add_argument("--output", type=Path, default=Path("build/release"))
    checksums = commands.add_parser("checksums")
    checksums.add_argument("tag")
    checksums.add_argument("--output", type=Path, default=Path("build/release"))
    args = parser.parse_args()
    try:
        version = release_version(ROOT, args.tag)
        if args.command == "check":
            print(f"version={version}")
            print(f"prerelease={str('-' in version).lower()}")
        elif args.command == "package":
            print(package_binary(ROOT, args.binary, args.platform, version, args.output))
        else:
            print(write_checksums(args.output, version))
    except (ValueError, OSError) as error:
        parser.exit(1, f"release: {error}\n")


if __name__ == "__main__":
    main()
