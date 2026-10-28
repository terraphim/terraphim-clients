#!/usr/bin/env python3
"""Validate a staged release binary's target ABI and Linux strip contract."""

from __future__ import annotations

import pathlib
import struct
import subprocess
import sys
from typing import NoReturn


TARGETS = {
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "universal-apple-darwin",
    "x86_64-pc-windows-msvc",
}

CPU_NAMES = {
    0x01000007: "x86_64",
    0x0100000C: "arm64",
}
THIN_MAGICS = {
    b"\xfe\xed\xfa\xce": (">", 32),
    b"\xce\xfa\xed\xfe": ("<", 32),
    b"\xfe\xed\xfa\xcf": (">", 64),
    b"\xcf\xfa\xed\xfe": ("<", 64),
}
FAT_MAGICS = {
    b"\xca\xfe\xba\xbe": (">", 32),
    b"\xbe\xba\xfe\xca": ("<", 32),
    b"\xca\xfe\xba\xbf": (">", 64),
    b"\xbf\xba\xfe\xca": ("<", 64),
}


def fail(message: str) -> NoReturn:
    raise SystemExit(f"ERROR: {message}")


def output(*command: str) -> str:
    try:
        return subprocess.run(
            command, text=True, capture_output=True, check=True, timeout=30
        ).stdout
    except (OSError, subprocess.CalledProcessError) as error:
        fail(f"command failed: {' '.join(command)}: {error}")


def parse_thin_macho(payload: bytes, context: str) -> str:
    details = THIN_MAGICS.get(payload[:4])
    if details is None:
        fail(f"{context}: slice is not a Mach-O image")
    endian, bits = details
    header_size = 32 if bits == 64 else 28
    if len(payload) < header_size:
        fail(f"{context}: truncated {bits}-bit Mach-O header")
    cpu_type = struct.unpack_from(f"{endian}i", payload, 4)[0]
    architecture = CPU_NAMES.get(cpu_type)
    if architecture is None:
        fail(f"{context}: unsupported Mach-O CPU type 0x{cpu_type & 0xffffffff:08x}")
    load_count, load_size = struct.unpack_from(f"{endian}II", payload, 16)
    load_end = header_size + load_size
    if load_end > len(payload):
        fail(f"{context}: truncated Mach-O load-command region")
    cursor = header_size
    for index in range(load_count):
        if cursor + 8 > load_end:
            fail(f"{context}: truncated Mach-O load command {index}")
        command_size = struct.unpack_from(f"{endian}I", payload, cursor + 4)[0]
        if command_size < 8 or command_size % 4 != 0 or cursor + command_size > load_end:
            fail(f"{context}: malformed Mach-O load command {index}")
        cursor += command_size
    if cursor != load_end:
        fail(f"{context}: Mach-O load-command count/size mismatch")
    return architecture


def parse_macho(payload: bytes) -> tuple[str, set[str]]:
    if len(payload) < 4:
        fail("truncated Mach-O magic")
    if payload[:4] in THIN_MAGICS:
        return "thin", {parse_thin_macho(payload, "thin image")}
    details = FAT_MAGICS.get(payload[:4])
    if details is None:
        fail("unrecognized Mach-O/fat magic")
    if len(payload) < 8:
        fail("truncated fat Mach-O header")
    endian, bits = details
    slice_count = struct.unpack_from(f"{endian}I", payload, 4)[0]
    if slice_count == 0 or slice_count > 32:
        fail(f"invalid fat Mach-O slice count {slice_count}")
    entry_size = 32 if bits == 64 else 20
    table_end = 8 + slice_count * entry_size
    if table_end > len(payload):
        fail("truncated fat Mach-O architecture table")
    architectures: set[str] = set()
    ranges: list[tuple[int, int]] = []
    for index in range(slice_count):
        entry = 8 + index * entry_size
        cpu_type = struct.unpack_from(f"{endian}i", payload, entry)[0]
        architecture = CPU_NAMES.get(cpu_type)
        if architecture is None:
            fail(f"fat slice {index}: unsupported CPU type 0x{cpu_type & 0xffffffff:08x}")
        if architecture in architectures:
            fail(f"fat slice {index}: duplicate {architecture} architecture")
        if bits == 64:
            offset, size, alignment, reserved = struct.unpack_from(
                f"{endian}QQII", payload, entry + 8
            )
            if reserved != 0:
                fail(f"fat slice {index}: reserved field is nonzero")
        else:
            offset, size, alignment = struct.unpack_from(
                f"{endian}III", payload, entry + 8
            )
        if size == 0 or offset < table_end or offset + size > len(payload):
            fail(f"fat slice {index}: invalid or truncated byte range")
        if alignment > 31 or offset % (1 << alignment) != 0:
            fail(f"fat slice {index}: invalid alignment")
        current = (offset, offset + size)
        if any(current[0] < end and start < current[1] for start, end in ranges):
            fail(f"fat slice {index}: overlapping byte range")
        actual = parse_thin_macho(payload[offset : offset + size], f"fat slice {index}")
        if actual != architecture:
            fail(
                f"fat slice {index}: table declares {architecture} but slice is {actual}"
            )
        architectures.add(architecture)
        ranges.append(current)
    return "fat", architectures


def validate_binary(path: pathlib.Path, target: str) -> None:
    if target not in TARGETS:
        fail(f"unsupported target: {target}")
    if not path.is_file() or path.is_symlink() or path.stat().st_size <= 0:
        fail(f"binary is not a non-empty regular file: {path}")
    if target.endswith("apple-darwin"):
        try:
            kind, architectures = parse_macho(path.read_bytes())
        except OSError as error:
            fail(f"cannot read Mach-O binary {path}: {error}")
        expected = {
            "x86_64-apple-darwin": ("thin", {"x86_64"}),
            "aarch64-apple-darwin": ("thin", {"arm64"}),
            "universal-apple-darwin": ("fat", {"x86_64", "arm64"}),
        }[target]
        if (kind, architectures) != expected:
            fail(
                f"{target} Mach-O identity {(kind, sorted(architectures))!r} "
                f"!= {(expected[0], sorted(expected[1]))!r}"
            )
        return
    description = output("file", "--brief", str(path)).strip()
    if target.startswith(("x86_64-unknown-linux", "aarch64-unknown-linux")):
        architecture = "x86-64" if target.startswith("x86_64") else "aarch64"
        if "ELF" not in description or architecture not in description:
            fail(f"{target} architecture mismatch; file output {description!r}")
        program_headers = output("readelf", "-l", str(path))
        sections = output("readelf", "-S", str(path))
        if ".symtab" in sections:
            fail(f"{target} final binary retains a .symtab section")
        interpreter_lines = [line for line in program_headers.splitlines() if "interpreter:" in line]
        interpreter = "\n".join(interpreter_lines)
        if target.endswith("-gnu"):
            if "ld-linux" not in interpreter or "ld-musl" in interpreter:
                fail(f"{target} must use the GNU loader")
        elif interpreter_lines:
            expected = "ld-musl-x86_64.so.1" if target.startswith("x86_64") else "ld-musl-aarch64.so.1"
            if expected not in interpreter:
                fail(f"{target} must use the target MUSL loader or be static PIE")
        elif "static" not in description.lower():
            fail(f"{target} has neither a MUSL loader nor a static/static-PIE file identity")
        return
    if "PE32+" not in description or "x86-64" not in description:
        fail(f"{target} architecture mismatch; file output {description!r}")


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: validate_release_binary.py TARGET BINARY")
    validate_binary(pathlib.Path(sys.argv[2]), sys.argv[1])


if __name__ == "__main__":
    main()
