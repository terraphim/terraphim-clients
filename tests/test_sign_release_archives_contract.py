import base64
import os
import subprocess
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "sign-release-archives.sh"


class SignReleaseArchivesContract(unittest.TestCase):
    def test_public_key_only_verifies_tar_and_zip_final_archives(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            payload = root / "binary"
            payload.write_bytes(b"executable")
            with tarfile.open(artifacts / "client.tar.gz", "w:gz") as bundle:
                bundle.add(payload, arcname="binary")
            with zipfile.ZipFile(artifacts / "client.zip", "w") as bundle:
                bundle.write(payload, "binary")

            private_key = root / "private.key"
            public_key = root / "public.key"
            subprocess.run(
                ["zipsign", "gen-key", str(private_key), str(public_key)],
                check=True,
                capture_output=True,
            )
            env = os.environ.copy()
            for archive, format_name in ((artifacts / "client.tar.gz", "tar"), (artifacts / "client.zip", "zip")):
                subprocess.run(
                    ["zipsign", "sign", format_name, str(archive), str(private_key)], check=True
                )
            env["ZIPSIGN_PUBLIC_KEY"] = base64.b64encode(public_key.read_bytes()).decode()
            verified = subprocess.run(
                [str(SCRIPT), "--verify-only", str(artifacts)],
                env=env,
                text=True,
                capture_output=True,
            )
            self.assertEqual(verified.returncode, 0, verified.stderr)
            self.assertIn("Verified 2 archive(s)", verified.stdout)

    def test_mismatched_private_key_fails_before_any_signing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifacts = root / "artifacts"
            artifacts.mkdir()
            archive = artifacts / "client.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("binary", b"payload")
            before = archive.read_bytes()
            private_key = root / "private.key"
            public_key = root / "public.key"
            subprocess.run(
                ["zipsign", "gen-key", str(private_key), str(public_key)], check=True,
                capture_output=True,
            )
            env = os.environ.copy()
            env["ZIPSIGN_PRIVATE_KEY"] = base64.b64encode(private_key.read_bytes()).decode()
            result = subprocess.run(
                [str(SCRIPT), str(artifacts)], env=env, text=True, capture_output=True
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("EMBEDDED_PUBLIC_KEYS[0]", result.stderr)
            self.assertEqual(archive.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
