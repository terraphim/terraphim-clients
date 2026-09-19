import io
import os
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "validate-release-archive.py"
BINARY_SCRIPT = ROOT / "scripts" / "validate_release_binary.py"
STAGE_SCRIPT = ROOT / "scripts" / "stage-canonical-linux.py"
ZIP_SCRIPT = ROOT / "scripts" / "create-deterministic-zip.py"


def write_tar(
    path: Path,
    *,
    mode: int = 0o755,
    extra: bool = False,
    duplicate: bool = False,
    binary_payload: bytes | None = None,
) -> None:
    entries = [
        ("terraphim-agent", binary_payload or Path("/bin/true").read_bytes(), mode),
        ("LICENSE-Apache-2.0", b"apache", 0o644),
        ("LICENSE-MIT", b"mit", 0o644),
    ]
    if extra:
        entries.append(("unexpected", b"extra", 0o644))
    if duplicate:
        entries.append(("LICENSE-MIT", b"duplicate", 0o644))
    with tarfile.open(path, "w:gz") as bundle:
        for name, payload, permissions in entries:
            info = tarfile.TarInfo(name)
            info.size = len(payload)
            info.mode = permissions
            bundle.addfile(info, io.BytesIO(payload))


def write_zip(
    path: Path, *, executable_mode: int = 0o755, license_mode: int = 0o644,
    duplicate: bool = False, symlink: bool = False, traversal: bool = False
) -> None:
    names = ["terraphim-agent.exe", "LICENSE-Apache-2.0", "LICENSE-MIT"]
    if traversal:
        names[-1] = "../LICENSE-MIT"
    if duplicate:
        names.append("LICENSE-MIT")
    with zipfile.ZipFile(path, "w") as bundle:
        for name in names:
            info = zipfile.ZipInfo(name)
            info.create_system = 3
            mode = executable_mode if name.endswith(".exe") else license_mode
            file_type = stat.S_IFLNK if symlink and name.endswith(".exe") else stat.S_IFREG
            info.external_attr = (file_type | mode) << 16
            bundle.writestr(info, b"payload")


def install_probe_tools(root: Path) -> Path:
    tools = root / "tools"
    tools.mkdir()
    for name, body in {
        "file": "#!/bin/sh\nprintf '%s\\n' \"$FILE_DESCRIPTION\"\n",
        "readelf": "#!/bin/sh\ncase \"$1\" in -l) printf '%s\\n' \"$READELF_HEADERS\";; -S) printf '%s\\n' \"$READELF_SECTIONS\";; esac\n",
    }.items():
        path = tools / name
        path.write_text(body)
        path.chmod(0o755)
    return tools


CPU_X86_64 = 0x01000007
CPU_ARM64 = 0x0100000C


def macho_thin(cpu: int, *, endian: str = "<", bits: int = 64) -> bytes:
    magic = 0xFEEDFACF if bits == 64 else 0xFEEDFACE
    fields = (magic, cpu, 3, 2, 0, 0, 0)
    header = struct.pack(f"{endian}IiiIIII", *fields)
    if bits == 64:
        header += struct.pack(f"{endian}I", 0)
    return header + b"\0" * 64


def macho_fat(
    slices: list[tuple[int, bytes]], *, endian: str = ">", bits: int = 32
) -> bytes:
    magic = 0xCAFEBABF if bits == 64 else 0xCAFEBABE
    entry_size = 32 if bits == 64 else 20
    table_end = 8 + len(slices) * entry_size
    offset = (table_end + 0xFFF) & ~0xFFF
    entries = []
    payload = bytearray(offset)
    for cpu, image in slices:
        if bits == 64:
            entries.append(struct.pack(f"{endian}iiQQII", cpu, 3, offset, len(image), 12, 0))
        else:
            entries.append(struct.pack(f"{endian}iiIII", cpu, 3, offset, len(image), 12))
        payload.extend(image)
        offset += len(image)
        aligned = (offset + 0xFFF) & ~0xFFF
        payload.extend(b"\0" * (aligned - offset))
        offset = aligned
    payload[:8] = struct.pack(f"{endian}II", magic, len(slices))
    payload[8:table_end] = b"".join(entries)
    return bytes(payload)


class ReleaseArchiveValidationContract(unittest.TestCase):
    def test_deterministic_zip_has_stable_bytes_and_executable_mode(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "package"
            package.mkdir()
            (package / "client.exe").write_bytes(b"executable")
            (package / "client.exe").chmod(0o755)
            (package / "LICENSE-Apache-2.0").write_bytes(b"apache")
            (package / "LICENSE-MIT").write_bytes(b"mit")
            first = root / "first.zip"
            second = root / "second.zip"
            command = [
                "python3",
                str(ZIP_SCRIPT),
                "1789689600",
                str(package),
            ]
            for output in (first, second):
                result = subprocess.run(
                    [*command, str(output), "client.exe", "LICENSE-Apache-2.0", "LICENSE-MIT"],
                    text=True,
                    capture_output=True,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            with __import__("zipfile").ZipFile(first) as bundle:
                info = bundle.getinfo("client.exe")
                self.assertEqual((info.external_attr >> 16) & 0o777, 0o755)

    def test_rejects_wrong_architecture_mode_layout_and_duplicate_members(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            valid = root / "terraphim-agent-1.21.15-x86_64-unknown-linux-gnu.tar.gz"
            write_tar(valid)
            accepted = subprocess.run(
                ["python3", str(SCRIPT), "1.21.15", str(valid)],
                text=True,
                capture_output=True,
            )
            self.assertEqual(accepted.returncode, 0, accepted.stderr)

            cases = {
                "wrong-architecture": (
                    root / "terraphim-agent-1.21.15-aarch64-unknown-linux-musl.tar.gz",
                    {},
                ),
                "non-executable": (
                    root / "terraphim-agent-1.21.15-x86_64-unknown-linux-musl.tar.gz",
                    {"mode": 0o644},
                ),
                "extra-layout": (
                    root / "terraphim-agent-1.21.15-x86_64-apple-darwin.tar.gz",
                    {"extra": True},
                ),
                "duplicate-member": (
                    root / "terraphim-agent-1.21.15-aarch64-apple-darwin.tar.gz",
                    {"duplicate": True},
                ),
            }
            for name, (archive, options) in cases.items():
                with self.subTest(name=name):
                    write_tar(archive, **options)
                    rejected = subprocess.run(
                        ["python3", str(SCRIPT), "1.21.15", str(archive)],
                        text=True,
                        capture_output=True,
                    )
                    self.assertNotEqual(rejected.returncode, 0)

    def test_validator_accepts_the_exact_post_sign_archive_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "terraphim-agent-1.21.15-x86_64-unknown-linux-gnu.tar.gz"
            write_tar(archive)
            private_key = root / "private.key"
            public_key = root / "public.key"
            subprocess.run(
                ["zipsign", "gen-key", str(private_key), str(public_key)],
                check=True,
                capture_output=True,
                timeout=30,
            )
            subprocess.run(
                ["zipsign", "sign", "tar", str(archive), str(private_key)],
                check=True,
                capture_output=True,
                timeout=30,
            )
            result = subprocess.run(
                ["python3", str(SCRIPT), "1.21.15", str(archive)],
                text=True,
                capture_output=True,
                timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stderr)

            macos = root / "terraphim-agent-1.21.15-x86_64-apple-darwin.tar.gz"
            write_tar(macos, binary_payload=macho_thin(CPU_X86_64))
            subprocess.run(
                ["zipsign", "sign", "tar", str(macos), str(private_key)],
                check=True,
                capture_output=True,
                timeout=30,
            )
            env = os.environ.copy()
            env["PATH"] = str(install_probe_tools(root))  # no lipo on this Ubuntu-style PATH
            mac_result = subprocess.run(
                [sys.executable, str(SCRIPT), "1.21.15", str(macos)],
                env=env,
                text=True,
                capture_output=True,
                timeout=30,
            )
            self.assertEqual(mac_result.returncode, 0, mac_result.stderr)

    def test_zip_and_tar_apply_equal_regular_file_path_and_exact_mode_rules(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            env = os.environ.copy()
            env.update({
                "PATH": f"{install_probe_tools(root)}:{env['PATH']}",
                "FILE_DESCRIPTION": "PE32+ executable x86-64",
                "READELF_HEADERS": "",
                "READELF_SECTIONS": "",
                "LIPO_ARCHS": "",
            })
            cases = (
                ("zip-exec-mode", {"executable_mode": 0o775}),
                ("zip-license-mode", {"license_mode": 0o600}),
                ("zip-duplicate", {"duplicate": True}),
                ("zip-symlink", {"symlink": True}),
                ("zip-traversal", {"traversal": True}),
            )
            for name, options in cases:
                with self.subTest(name=name):
                    archive = root / "terraphim-agent-1.21.15-x86_64-pc-windows-msvc.zip"
                    write_zip(archive, **options)
                    result = subprocess.run(
                        ["python3", str(SCRIPT), "1.21.15", str(archive)],
                        env=env, text=True, capture_output=True,
                    )
                    self.assertNotEqual(result.returncode, 0)

            tar_path = root / "terraphim-agent-1.21.15-x86_64-unknown-linux-gnu.tar.gz"
            write_tar(tar_path, mode=0o775)
            result = subprocess.run(
                ["python3", str(SCRIPT), "1.21.15", str(tar_path)],
                text=True, capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)

    def test_binary_validator_distinguishes_abi_fatness_and_strip_state(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "binary"
            binary.write_bytes(b"fixture")
            tools = install_probe_tools(root)
            base = os.environ.copy()
            base["PATH"] = f"{tools}:{base['PATH']}"

            cases = (
                ("gnu-ok", "x86_64-unknown-linux-gnu", "ELF x86-64 dynamically linked", "interpreter: /lib64/ld-linux-x86-64.so.2", "", "", True),
                ("gnu-musl-loader", "x86_64-unknown-linux-gnu", "ELF x86-64 dynamically linked", "interpreter: /lib/ld-musl-x86_64.so.1", "", "", False),
                ("musl-static-pie", "x86_64-unknown-linux-musl", "ELF x86-64 static-pie linked", "", "", "", True),
                ("musl-gnu-loader", "x86_64-unknown-linux-musl", "ELF x86-64 dynamically linked", "interpreter: /lib64/ld-linux-x86-64.so.2", "", "", False),
                ("unstripped", "x86_64-unknown-linux-gnu", "ELF x86-64 dynamically linked", "interpreter: /lib64/ld-linux-x86-64.so.2", ".symtab", "", False),
            )
            for name, target, desc, headers, sections, archs, accepted in cases:
                with self.subTest(name=name):
                    env = base | {
                        "FILE_DESCRIPTION": desc,
                        "READELF_HEADERS": headers,
                        "READELF_SECTIONS": sections,
                        "LIPO_ARCHS": archs,
                    }
                    result = subprocess.run(
                        ["python3", str(BINARY_SCRIPT), target, str(binary)],
                        env=env, text=True, capture_output=True,
                    )
                    self.assertEqual(result.returncode == 0, accepted, result.stderr)

    def test_macho_validation_is_host_independent_and_exact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tools = install_probe_tools(root)
            env = os.environ.copy()
            env["PATH"] = str(tools)  # deliberately excludes lipo and all host tools

            x86 = macho_thin(CPU_X86_64)
            arm = macho_thin(CPU_ARM64, endian=">")
            universal = macho_fat([(CPU_X86_64, x86), (CPU_ARM64, arm)])
            universal_le64 = macho_fat(
                [(CPU_ARM64, macho_thin(CPU_ARM64)), (CPU_X86_64, x86)],
                endian="<",
                bits=64,
            )
            universal_le32 = macho_fat(
                [(CPU_X86_64, x86), (CPU_ARM64, macho_thin(CPU_ARM64))],
                endian="<",
            )
            universal_be64 = macho_fat(
                [(CPU_X86_64, macho_thin(CPU_X86_64, endian=">")), (CPU_ARM64, arm)],
                bits=64,
            )
            fixtures = (
                ("x86-thin", "x86_64-apple-darwin", x86, True),
                ("arm-thin", "aarch64-apple-darwin", arm, True),
                ("x86-thin-big", "x86_64-apple-darwin", macho_thin(CPU_X86_64, endian=">"), True),
                ("arm-thin-little", "aarch64-apple-darwin", macho_thin(CPU_ARM64), True),
                ("universal", "universal-apple-darwin", universal, True),
                ("universal-fat64-little", "universal-apple-darwin", universal_le64, True),
                ("universal-fat32-little", "universal-apple-darwin", universal_le32, True),
                ("universal-fat64-big", "universal-apple-darwin", universal_be64, True),
                ("thin-rejects-fat", "x86_64-apple-darwin", universal, False),
                ("fat-missing", "universal-apple-darwin", macho_fat([(CPU_ARM64, arm)]), False),
                ("fat-extra", "universal-apple-darwin", macho_fat([(CPU_X86_64, x86), (CPU_ARM64, arm), (0x12, macho_thin(0x12))]), False),
                ("fat-duplicate", "universal-apple-darwin", macho_fat([(CPU_ARM64, arm), (CPU_ARM64, arm)]), False),
                ("wrong-thin", "x86_64-apple-darwin", arm, False),
                ("32-bit-thin", "x86_64-apple-darwin", macho_thin(7, bits=32), False),
                ("mixed-declaration", "universal-apple-darwin", macho_fat([(CPU_X86_64, arm), (CPU_ARM64, x86)]), False),
                ("truncated-thin", "aarch64-apple-darwin", arm[:12], False),
                ("truncated-fat", "universal-apple-darwin", universal[:30], False),
                ("malformed", "universal-apple-darwin", b"not-mach-o", False),
            )
            for name, target, payload, accepted in fixtures:
                with self.subTest(name=name):
                    binary = root / name
                    binary.write_bytes(payload)
                    result = subprocess.run(
                        [sys.executable, str(BINARY_SCRIPT), target, str(binary)],
                        env=env,
                        text=True,
                        capture_output=True,
                    )
                    self.assertEqual(result.returncode == 0, accepted, result.stderr)

    def test_canonical_linux_stage_rejects_unstripped_and_hashes_exact_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "raw"
            raw.mkdir()
            targets = (
                "aarch64-unknown-linux-musl", "x86_64-unknown-linux-gnu",
                "x86_64-unknown-linux-musl",
            )
            binaries = ("terraphim-agent", "terraphim-cli", "terraphim-grep")
            for target in targets:
                for binary in binaries:
                    (raw / f"{binary}-{target}").write_bytes(f"post-strip:{binary}:{target}".encode())
            tools = root / "stage-tools"
            tools.mkdir()
            probes = {
                "file": "#!/bin/sh\ncase \"$2\" in *aarch64*) echo 'ELF aarch64 static-pie linked';; *musl*) echo 'ELF x86-64 static-pie linked';; *) echo 'ELF x86-64 dynamically linked';; esac\n",
                "readelf": "#!/bin/sh\nif [ \"$1\" = -S ]; then printf '%s\\n' \"${READELF_SECTIONS:-}\"; elif echo \"$2\" | grep -q gnu; then echo 'interpreter: /lib64/ld-linux-x86-64.so.2'; fi\n",
            }
            for name, body in probes.items():
                path = tools / name
                path.write_text(body)
                path.chmod(0o755)
            env = os.environ.copy()
            env["PATH"] = f"{tools}:{env['PATH']}"

            rejected_env = env | {"READELF_SECTIONS": ".symtab"}
            rejected = subprocess.run(
                [str(STAGE_SCRIPT), str(raw), str(root / "rejected"), str(root / "bad-sums")],
                env=rejected_env, text=True, capture_output=True,
            )
            self.assertNotEqual(rejected.returncode, 0)
            self.assertFalse((root / "rejected").exists())

            staged = root / "canonical"
            sums = root / "BINARY_SHA256SUMS"
            accepted = subprocess.run(
                [str(STAGE_SCRIPT), str(raw), str(staged), str(sums)],
                env=env, text=True, capture_output=True,
            )
            self.assertEqual(accepted.returncode, 0, accepted.stderr)
            rows = dict(line.split("  ", 1) for line in sums.read_text().splitlines())
            self.assertEqual(set(rows.values()), {path.name for path in raw.iterdir()})
            for digest, name in rows.items():
                self.assertEqual((raw / name).read_bytes(), (staged / name).read_bytes())
                self.assertEqual(digest, __import__("hashlib").sha256((staged / name).read_bytes()).hexdigest())


if __name__ == "__main__":
    unittest.main()
