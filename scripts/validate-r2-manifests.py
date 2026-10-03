#!/usr/bin/env python3
"""Read-only migration-aware validation of release manifests and asset bytes."""

from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import re
import urllib.error
import urllib.parse
import urllib.request


COMMON = {
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
}
EXPECTED_TARGETS = {
    "terraphim-agent": COMMON | {"universal-apple-darwin"},
    "terraphim-grep": COMMON | {"universal-apple-darwin"},
    "terraphim-cli": COMMON,
}
SEMVER = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def version_tuple(value: str) -> tuple[int, int, int]:
    match = SEMVER.fullmatch(value) if isinstance(value, str) else None
    if match is None:
        raise ValueError(f"invalid stable version {value!r}")
    return tuple(map(int, match.groups()))


def fetch(base_url: str, path: str, limit: int) -> bytes:
    # Cloudflare bot management on the public channel answers the default
    # Python-urllib User-Agent with 403, so identify as the validator.
    request = urllib.request.Request(  # nosec B310: validate() allowlists the scheme
        f"{base_url}/{path}",
        headers={"User-Agent": "terraphim-r2-manifest-validator/1.0"},
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError(f"{path}: response exceeds {limit} bytes")
    return data


def load_manifest(base_url: str, path: str) -> dict:
    return json.loads(fetch(base_url, path, 1_048_576), object_pairs_hook=unique_object)


def validate_strict_metadata(binary: str, manifest: dict) -> tuple[str, set[str]]:
    if not isinstance(manifest, dict) or set(manifest) != {"version", "released_at", "assets", "notes_url"}:
        raise ValueError(f"{binary}: manifest top-level keys are not exact")
    version = manifest["version"]
    version_tuple(version)
    if not isinstance(manifest["assets"], dict) or set(manifest["assets"]) != EXPECTED_TARGETS[binary]:
        raise ValueError(f"{binary}: target set is not exact")
    if not isinstance(manifest["notes_url"], str) or urllib.parse.urlsplit(manifest["notes_url"]).scheme != "https":
        raise ValueError(f"{binary}: notes_url is not HTTPS")
    if not isinstance(manifest["released_at"], str) or not re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ", manifest["released_at"]):
        raise ValueError(f"{binary}: released_at is not UTC RFC3339 seconds")
    return version, EXPECTED_TARGETS[binary]


def expected_path(binary: str, version: str, target: str) -> str:
    extension = ".zip" if target == "x86_64-pc-windows-msvc" else ".tar.gz"
    return f"{binary}/{binary}-{version}-{target}{extension}"


def validate_legacy(binary: str, manifest: dict, require_exact: bool) -> str:
    if not isinstance(manifest, dict):
        raise ValueError(f"{binary}: legacy manifest is not an object")
    required = {"version", "released_at", "assets"}
    allowed = required | {"notes_url"}
    if not required <= set(manifest) or not set(manifest) <= allowed:
        raise ValueError(f"{binary}: legacy manifest keys are invalid")
    if require_exact and set(manifest) != allowed:
        raise ValueError(f"{binary}: legacy manifest keys are not exact after activation")
    version = manifest["version"]
    version_tuple(version)
    assets = manifest["assets"]
    if not isinstance(assets, dict) or not assets:
        raise ValueError(f"{binary}: legacy assets are not a non-empty object")
    if not isinstance(manifest["released_at"], str) or not re.fullmatch(
        r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ", manifest["released_at"]
    ):
        raise ValueError(f"{binary}: released_at is not UTC RFC3339 seconds")
    notes_url = manifest.get("notes_url")
    if notes_url is not None and (
        not isinstance(notes_url, str)
        or urllib.parse.urlsplit(notes_url).scheme != "https"
        or not urllib.parse.urlsplit(notes_url).netloc
    ):
        raise ValueError(f"{binary}: notes_url is not absolute HTTPS")
    for target, path in assets.items():
        if not isinstance(target, str) or not re.fullmatch(r"[A-Za-z0-9_.+-]+", target):
            raise ValueError(f"{binary}: unsafe legacy target key")
        if not isinstance(path, str):
            raise ValueError(f"{binary}/{target}: legacy asset path is not a string")
        parsed = urllib.parse.urlsplit(path)
        parts = path.split("/")
        if (
            parsed.scheme
            or parsed.netloc
            or parsed.query
            or parsed.fragment
            or parsed.path != path
            or path.startswith("/")
            or "\\" in path
            or any(part in {"", ".", ".."} for part in parts)
            or parts[0] != binary
        ):
            raise ValueError(f"{binary}/{target}: unsafe legacy asset path")
    if require_exact and set(assets) != EXPECTED_TARGETS[binary]:
        raise ValueError(f"{binary}: legacy target set is not exact after activation")
    for target in EXPECTED_TARGETS[binary] if require_exact else ():
        path = manifest["assets"][target]
        if path != expected_path(binary, version, target):
            raise ValueError(f"{binary}/{target}: legacy asset path mismatch")
    return version


def verify_asset(
    base_url: str,
    path: str,
    declared_size: int,
    expected_sha256: str,
    *,
    chunk_size: int = 65_536,
    opener=urllib.request.urlopen,
) -> None:
    if chunk_size <= 0:
        raise ValueError("chunk size must be positive")
    digest = hashlib.sha256()
    total = 0
    request = urllib.request.Request(  # nosec B310: validate() allowlists the scheme
        f"{base_url}/{path}",
        headers={"User-Agent": "terraphim-r2-manifest-validator/1.0"},
    )
    with opener(request, timeout=60) as response:
        while True:
            chunk = response.read(min(chunk_size, declared_size - total + 1))
            if not chunk:
                break
            total += len(chunk)
            if total > declared_size:
                raise ValueError(f"{path}: size exceeds declared {declared_size} bytes")
            digest.update(chunk)
    if total != declared_size:
        raise ValueError(f"{path}: size mismatch (declared {declared_size}, received {total})")
    if not hmac.compare_digest(digest.hexdigest(), expected_sha256):
        raise ValueError(f"{path}: checksum mismatch")


def validate_strict(base_url: str, binary: str, manifest: dict) -> str:
    version, targets = validate_strict_metadata(binary, manifest)
    for target in sorted(targets):
        asset = manifest["assets"][target]
        if not isinstance(asset, dict) or set(asset) != {"path", "sha256", "size"}:
            raise ValueError(f"{binary}/{target}: strict asset keys are not exact")
        if asset["path"] != expected_path(binary, version, target):
            raise ValueError(f"{binary}/{target}: filename/version mismatch")
        if not isinstance(asset["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", asset["sha256"]):
            raise ValueError(f"{binary}/{target}: invalid sha256")
        if isinstance(asset["size"], bool) or not isinstance(asset["size"], int) or asset["size"] <= 0:
            raise ValueError(f"{binary}/{target}: invalid size")
        verify_asset(base_url, asset["path"], asset["size"], asset["sha256"])
    return version


def validate(base_url: str, activation: str) -> None:
    if urllib.parse.urlsplit(base_url).scheme not in {"https", "file"}:
        raise ValueError("base URL scheme must be HTTPS (or file for local fixtures)")
    activation_version = version_tuple(activation)
    for binary in EXPECTED_TARGETS:
        legacy = load_manifest(base_url, f"{binary}/stable.json")
        if not isinstance(legacy, dict) or "version" not in legacy:
            raise ValueError(f"{binary}: legacy manifest has no version")
        require_exact_legacy = version_tuple(legacy["version"]) >= activation_version
        legacy_version = validate_legacy(binary, legacy, require_exact_legacy)
        try:
            strict = load_manifest(base_url, f"{binary}/stable-v2.json")
        except urllib.error.HTTPError as error:
            if error.code != 404:
                raise
            strict = None
        except urllib.error.URLError as error:
            if not isinstance(error.reason, FileNotFoundError):
                raise
            strict = None
        if strict is None:
            if version_tuple(legacy_version) >= activation_version:
                raise ValueError(f"{binary}: stable-v2.json is mandatory at {legacy_version}")
            print(f"migration-pending {binary}: legacy {legacy_version}")
            continue
        strict_version = validate_strict(base_url, binary, strict)
        if version_tuple(legacy_version) >= activation_version:
            if strict_version != legacy_version:
                raise ValueError(f"{binary}: stable and stable-v2 versions differ after activation")
            if (
                legacy["released_at"] != strict["released_at"]
                or legacy["notes_url"] != strict["notes_url"]
                or any(
                    legacy["assets"][target] != strict["assets"][target]["path"]
                    for target in EXPECTED_TARGETS[binary]
                )
            ):
                raise ValueError(f"{binary}: legacy and strict metadata differ after activation")
        print(f"verified {binary}: legacy={legacy_version} strict={strict_version}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--activation-version", default="1.21.15")
    args = parser.parse_args()
    validate(args.base_url.rstrip("/"), args.activation_version)


if __name__ == "__main__":
    main()
