# terraphim-clients-bin (Arch / AUR / Omarchy)

Canonical split PKGBUILD for the prebuilt Terraphim AI client CLIs, published
from the public immutable GitHub release `terraphim/terraphim-clients`.

- **pkgbase:** `terraphim-clients-bin`
- **outputs:** `terraphim-agent-bin` (Apache-2.0) and `terraphim-grep-bin` (MIT)
- **architectures:** `x86_64`, `aarch64` (static MUSL binaries, no runtime deps)
- **tracks:** Gitea terraphim-clients#316 (AUR), #249 (Omarchy downstream)

Each split output owns exactly one executable, one license text and one
per-binary package-manager receipt:

| Owners | `terraphim-agent-bin` | `terraphim-grep-bin` |
|---|---|---|
| executable | `/usr/bin/terraphim-agent` | `/usr/bin/terraphim-grep` |
| license | `/usr/share/licenses/terraphim-agent-bin/LICENSE-Apache-2.0` | `/usr/share/licenses/terraphim-grep-bin/LICENSE-MIT` |
| receipt | `/usr/share/terraphim/package-manager.d/terraphim-agent` = `pacman` | `/usr/share/terraphim/package-manager.d/terraphim-grep` = `pacman` |

The receipt is what makes the packaged clients treat themselves as
package-managed: no startup update request and no self-update writing a shadow
binary under `~/.local/bin` (see `crates/terraphim_update/src/policy.rs`).

## Verification

`verify.sh` builds the split package as an unprivileged user, checks
`.SRCINFO` drift, runs `namcap`, asserts split ownership, then installs,
`pacman -Qkk`\ s, smoke-tests `--version` against `pkgver`, checks both
receipts and removes the packages.

Run it in an Arch container (Docker/OrbStack on macOS):

```bash
cd pkgbuilds/terraphim-clients-bin
docker run --rm -v "$PWD":/w -w /w archlinux:base-devel bash -lc \
  'pacman -Sy --noconfirm namcap >/dev/null && ./verify.sh'
```

```bash
python3 -m unittest tests.test_pkgbuild_release_contract -v
```

## Regenerating .SRCINFO

`.SRCINFO` is generated, never hand-edited:

```bash
docker run --rm -v "$PWD":/w -w /w archlinux:base-devel bash -lc \
  'makepkg --printsrcinfo' > .SRCINFO
```

## Releasing a new version

1. Confirm the new public GitHub release exists with all four MUSL archives.
2. Bump `pkgver` in `PKGBUILD`.
3. Copy the two x86_64 and two aarch64 SHA-256 values from the release's
   `SHA256SUMS` into the matching `sha256sums_*` arrays.
4. Update the pinned digests in `tests/test_pkgbuild_release_contract.py`.
5. Regenerate `.SRCINFO` and run `./verify.sh`.
6. AUR: clone `ssh://aur@aur.archlinux.org/terraphim-clients-bin.git`,
   copy `PKGBUILD` and `.SRCINFO` to the repo root, commit and push
   `master`. AUR is not on the Omarchy critical path.

## Downstream Omarchy

Omarchy consumes this package as one directory,
`omacom/omarchy-pkgs/pkgbuilds/terraphim-clients-bin/`, with
`.omarchy/package.json` using `"source": "local"`, `"release_ring": "fast"`
and an `upstream.watch.github` entry for `terraphim/terraphim-clients`
(see Gitea terraphim-clients#249). The split semantics above are the contract;
do not fork the PKGBUILD.
