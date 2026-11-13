"""Exercise the actual sealed stage and saved GitHub metadata; no substitute binaries."""

from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "pkgbuild_stage", ROOT / "scripts/prepare-pkgbuild-stage.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class SealedStageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Missing real artefacts is a failure, never a skipped acceptance test.
        cls.stage = Path(os.environ["PKGBUILD_STAGE_TEST_DIR"])
        cls.run_metadata = MODULE.read_json(Path(os.environ["PKGBUILD_STAGE_RUN_JSON"]))
        cls.artifact = MODULE.read_json(
            Path(os.environ["PKGBUILD_STAGE_ARTIFACT_JSON"])
        )
        cls.value = MODULE.contract(
            ROOT / "pkgbuilds/terraphim-clients-bin/release-stage.json"
        )
        cls.arch = {"arm64": "aarch64"}.get(platform.machine(), platform.machine())
        MODULE.validate_metadata(cls.value, cls.run_metadata, cls.artifact)

    def prepare(
        self, cache, *, value=None, run=None, artifact=None, stage=None, arch=None
    ):
        return MODULE.prepare(
            value or self.value,
            stage or self.stage,
            run or self.run_metadata,
            artifact or self.artifact,
            cache,
            arch or self.arch,
        )

    def test_actual_stage_two_archives_exact_readback(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = Path(tmp)
            receipt = self.prepare(cache)
            self.assertEqual(len(receipt["cached"]), 2)
            self.assertEqual(receipt["architecture"], self.arch)
            for name, digest in receipt["cached"].items():
                self.assertEqual(
                    (cache / name).read_bytes(),
                    (self.stage / "release-assets" / name).read_bytes(),
                )
                self.assertEqual(digest, self.value["files"]["release-assets/" + name])

    def test_reject_changed_run_identity_before_cache_write(self):
        mutations = [
            ("path", ".github/workflows/ci.yml"),
            ("head_sha", "f" * 40),
            ("head_branch", "unreviewed"),
            ("run_attempt", self.run_metadata["run_attempt"] + 1),
            ("id", self.run_metadata["id"] + 1),
            ("event", "pull_request"),
            ("status", "in_progress"),
            ("conclusion", "failure"),
            ("repository", {"full_name": "other/repo", "id": 1}),
            ("head_repository", {"full_name": "other/repo", "id": 1}),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            for key, changed in mutations:
                with self.subTest(field=key):
                    run = copy.deepcopy(self.run_metadata)
                    run[key] = changed
                    with self.assertRaises(ValueError):
                        self.prepare(Path(tmp), run=run)
                    self.assertEqual(list(Path(tmp).iterdir()), [])

    def test_reject_changed_artifact_identity_before_cache_write(self):
        mutations = [
            ("id", self.artifact["id"] + 1),
            ("name", "other"),
            ("digest", "sha256:" + "f" * 64),
            ("expired", True),
            ("workflow_run", {"id": 1}),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            for key, changed in mutations:
                with self.subTest(field=key):
                    artifact = copy.deepcopy(self.artifact)
                    artifact[key] = changed
                    with self.assertRaises(ValueError):
                        self.prepare(Path(tmp), artifact=artifact)
                    self.assertEqual(list(Path(tmp).iterdir()), [])

    def test_reject_wrong_native_architecture(self):
        with tempfile.TemporaryDirectory() as tmp:
            for arch in ("x86_64" if self.arch == "aarch64" else "aarch64", "i686"):
                with self.assertRaises(ValueError):
                    self.prepare(Path(tmp), arch=arch)
                self.assertEqual(list(Path(tmp).iterdir()), [])

    def copied_stage(self, root):
        # Callers create roots beside the stage so Arch container mount boundaries
        # cannot make hardlinks fail with EXDEV. Changed test files are unlinked
        # before replacement, so the retained original stage is never modified.
        return Path(
            shutil.copytree(
                self.stage, Path(root) / self.stage.name, copy_function=os.link
            )
        )

    def test_reject_missing_extra_corrupt_symlink_and_directory_entries(self):
        for kind in ("missing", "extra", "corrupt", "symlink", "directory"):
            with (
                self.subTest(kind=kind),
                tempfile.TemporaryDirectory(dir=self.stage.parent) as tmp,
            ):
                stage = self.copied_stage(tmp)
                cache = Path(tmp) / "cache"
                cache.mkdir()
                p = stage / "provenance.json"
                if kind == "missing":
                    p.unlink()
                elif kind == "extra":
                    (stage / "extra").write_text("unexpected")
                elif kind == "corrupt":
                    content = p.read_bytes()
                    p.unlink()
                    p.write_bytes(content + b"\n")
                elif kind == "symlink":
                    p.unlink()
                    p.symlink_to(self.stage / "provenance.json")
                else:
                    (stage / "unexpected-directory").mkdir()
                with self.assertRaises(ValueError):
                    self.prepare(cache, stage=stage)
                self.assertEqual(list(cache.iterdir()), [])

    def test_existing_promotion_validator_rejects_wrong_provenance(self):
        with tempfile.TemporaryDirectory(dir=self.stage.parent) as tmp:
            stage = self.copied_stage(tmp)
            cache = Path(tmp) / "cache"
            cache.mkdir()
            p = stage / "provenance.json"
            value = copy.deepcopy(self.value)
            provenance = MODULE.read_json(p)
            provenance["source_sha"] = "f" * 40
            p.unlink()
            p.write_text(json.dumps(provenance))
            value["files"]["provenance.json"] = hashlib.sha256(
                p.read_bytes()
            ).hexdigest()
            with self.assertRaises(SystemExit):
                self.prepare(cache, stage=stage, value=value)
            self.assertEqual(list(cache.iterdir()), [])

    def test_existing_cache_is_not_replaced(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "retained.tar.gz"
            p.write_bytes(b"existing user-selected cache")
            with self.assertRaises(ValueError):
                self.prepare(Path(tmp))
            self.assertEqual(p.read_bytes(), b"existing user-selected cache")
            self.assertEqual(list(Path(tmp).iterdir()), [p])

    def test_only_absence_allows_stage_mode(self):
        self.assertEqual(
            MODULE.release_mode(self.value, 404, {"message": "Not Found"}), "stage"
        )
        for status in (301, 401, 403, 429, 500):
            with self.assertRaises(ValueError):
                MODULE.release_mode(self.value, status, {})
        for release in (
            {},
            {
                "tag_name": "v" + self.value["version"],
                "draft": False,
                "prerelease": False,
                "assets": [],
            },
        ):
            with self.assertRaises(ValueError):
                MODULE.release_mode(self.value, 200, release)

    def test_contract_parser_rejects_unsafe_or_duplicate_fields(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "contract.json"
            p.write_text('{"version":"1","version":"2"}')
            with self.assertRaises(ValueError):
                MODULE.contract(p)
            for field, changed in [
                ("run_attempt", True),
                ("artifact_id", -1),
                ("artifact_name", "../escape"),
                ("files", {"../escape": "f" * 64}),
            ]:
                value = copy.deepcopy(self.value)
                value[field] = changed
                p.write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    MODULE.contract(p)


if __name__ == "__main__":
    unittest.main()
