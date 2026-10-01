"""Release boundary checks: version mismatches, ZIP contents, and missing assets."""

import hashlib
from pathlib import Path
import stat
import tempfile
import unittest
import zipfile

import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.set_version("0.1.0")

    def set_version(self, version, locked=None):
        (self.root / "Cargo.toml").write_text(
            f'[package]\nname = "typptx"\nversion = "{version}"\n', encoding="utf-8"
        )
        (self.root / "Cargo.lock").write_text(
            f'[[package]]\nname = "typptx"\nversion = "{locked or version}"\n', encoding="utf-8"
        )

    def test_version_matches_tag_and_lock(self):
        self.assertEqual(release.release_version(self.root, "v-0.1.0"), "0.1.0")
        self.set_version("0.2.0-rc.1")
        self.assertEqual(release.release_version(self.root, "v-0.2.0-rc.1"), "0.2.0-rc.1")

    def test_rejects_wrong_tag_and_stale_lock(self):
        for tag in ("v0.1.0", "v-0.8.0", "v-../0.1.0"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release.release_version(self.root, tag)
        self.set_version("0.1.0", locked="0.8.0")
        with self.assertRaisesRegex(ValueError, "Cargo.lock"):
            release.release_version(self.root, "v-0.1.0")

    def test_binary_zip_has_executable_and_documentation(self):
        binary = self.root / "binary"
        binary.write_bytes(b"binary contents")
        for platform in release.PLATFORMS:
            with self.subTest(platform=platform):
                archive = release.package_binary(release.ROOT, binary, platform, "0.1.0", self.root)
                self.assertEqual(archive.name, f"typptx-v0.1.0-{platform}.zip")
                executable = "typptx.exe" if platform.startswith("windows-") else "typptx"
                with zipfile.ZipFile(archive) as bundle:
                    self.assertIsNone(bundle.testzip())
                    self.assertEqual(bundle.read(executable), binary.read_bytes())
                    self.assertEqual(stat.S_IMODE(bundle.getinfo(executable).external_attr >> 16), 0o755)
                    self.assertEqual(bundle.read("LICENSE"), (release.ROOT / "LICENSE").read_bytes())
                    self.assertIn("docs/assets/logo.svg", bundle.namelist())
                    self.assertIn("licenses/HarfBuzz-COPYING", bundle.namelist())
                    self.assertEqual(len(bundle.namelist()), 6)

    def test_missing_binary_does_not_create_archive(self):
        with self.assertRaisesRegex(ValueError, "missing or empty"):
            release.package_binary(release.ROOT, self.root / "missing", "macos-arm64", "0.1.0", self.root)
        self.assertEqual(list(self.root.glob("*.zip")), [])

    def test_checksums_require_every_platform_and_source(self):
        with self.assertRaisesRegex(ValueError, "missing="):
            release.write_checksums(self.root, "0.1.0")
        names = ["typptx-v0.1.0.zip"]
        names.extend(f"typptx-v0.1.0-{platform}.zip" for platform in release.PLATFORMS)
        for name in names:
            (self.root / name).write_bytes(name.encode())
        checksums = release.write_checksums(self.root, "0.1.0")
        lines = checksums.read_text().splitlines()
        self.assertEqual(len(lines), 5)
        for line in lines:
            digest, name = line.split("  ")
            self.assertEqual(digest, hashlib.sha256((self.root / name).read_bytes()).hexdigest())
        (self.root / "typptx-v0.8.0.zip").write_bytes(b"stale")
        with self.assertRaisesRegex(ValueError, "unexpected="):
            release.write_checksums(self.root, "0.1.0")


if __name__ == "__main__":
    unittest.main()
