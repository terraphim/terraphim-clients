# Client release operator checklist

This checklist separates reproducible artifact production from privileged
publication. The `release-binaries.yml` producer has read-only repository
permissions and never creates or mutates a GitHub release or R2 object.

## 1. Authorize and dispatch staging

- [ ] Confirm the checked-in workspace version, requested version, `v` tag,
  peeled tag SHA, and `expected_source_sha` all agree.
- [ ] Use `target_repo=terraphim-ai` and
  `publish_to_target_release=false`; the latter is a compatibility input and
  any true value is rejected.
- [ ] Record the correlation ID and resulting run ID.
- [ ] Do not include DEB/RPM or nFPM outputs. They are built and reviewed in a
  separate worktree/release stream.

The only producer output is
`client-release-stage-<version>-<source-sha>`. Download it without merging it
with artifacts from another run or SHA.

The producer pins Rust `1.96.0`, zipsign `0.2.1`, cross commit
`88f49ff79e777bef6d3564531636ee4d3cc2f8d2`, and every action in a
secret-bearing job to a recorded 40-character commit. In particular,
`1password/install-cli-action` v2 tag object
`c1b138d5779f64eda6936d5caa8e754b9f3996c0` is peeled to commit
`9a0c9dd934086b7ab1d90115d455bda1c53c2bdb`. Tool installation steps receive
no registry or signing credentials.

## 2. Inspect the sealed stage

- [ ] `provenance.json` contains the requested version, tag, exact source SHA,
  correlation ID, exact `client-release-stage-<version>-<source-sha>` identity,
  and embedded zipsign signature scheme. Promotion checks this exact object;
  visual inspection is additional evidence, not the gate.
- [ ] `expected-assets.txt` lists exactly 20 archives: seven each for
  `terraphim-agent` and `terraphim-grep`, and six for `terraphim-cli`.
- [ ] Both Omarchy targets are present for agent and grep:
  `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`.
- [ ] `sha256sum -c SHA256SUMS` succeeds from `release-assets/`.
- [ ] `sha256sum -c ../BINARY_SHA256SUMS` succeeds from
  `canonical-binaries/`; these are the already-stripped Linux bytes that every
  downstream package producer must consume unchanged.
- [ ] Each archive has an embedded verified signature and contains exactly its
  executable, `LICENSE-Apache-2.0`, and `LICENSE-MIT`.
- [ ] The three `manifests/<binary>.v2.candidate.json` files parse with
  `python3 -m json.tool`, carry exact `path`, `sha256`, and positive `size`
  asset keys, and match `SHA256SUMS`. Each corresponding
  `<binary>.v1.candidate.json` contains the same metadata and exact target
  paths in the legacy string-valued shape.

Do not rename a candidate to either stable pointer manually. Candidate
generation refuses to write `stable.json` or `stable-v2.json` by design.

## 3. Obtain separate publication authorization

- [ ] Use Python 3.9 or newer. Both privileged entrypoints reject older
  interpreters before stage validation or any remote query.
- [ ] Obtain explicit authorization for GitHub/R2 mutation after the stage has
  been reviewed.
- [ ] Confirm the destination GitHub release exists and its tag is exact.
- [ ] Confirm the release is neither `draft` nor `prerelease`; either state is
  forbidden from advancing stable manifests.
- [ ] Configure `gh`, R2 credentials, and the public `BASE_URL` in the
  privileged operator environment. Exporting `R2_ENDPOINT`,
  `R2_ACCESS_KEY_ID`, and `R2_SECRET_ACCESS_KEY` uploads through the S3 API,
  which is the transport used by `promote-release.yml`; without them the
  script falls back to `wrangler r2 object put --remote`, which has reported
  success for multi-megabyte objects that never became readable. These
  credentials do not belong in the producer workflow.
- [ ] Set `TMPDIR` to a protected filesystem with enough space for one largest
  remote object plus the small plans. Successful comparison/readback copies are
  removed immediately rather than retained until process exit. The stage
  filesystem separately needs space for the retained six-pointer snapshot.

## 4. Promote immutable objects, then stable pointers

Keep the extracted directory basename unchanged and run from the repository:

```bash
scripts/promote-release.sh \
  1.21.15 \
  /absolute/path/to/client-release-stage-1.21.15-<40-hex-source-sha> \
  terraphim-clients \
  <40-hex-source-sha> \
  '<correlation-id>'
```

Before its first remote query, the command rejects missing/duplicate/mismatched
provenance, an unexpected stage directory identity, mixed candidates, or a
`SHA256SUMS` set that does not exactly bind the candidate assets. It then checks
the GitHub release state, uploads immutable release/R2 objects, downloads and
verifies every uploaded object, uploads versioned candidate manifests, captures
the complete pre-promotion state of all six pointers in
`rollback-pointers/`, and only then advances per-binary pointers in fleet-safe
order: strict `stable-v2.json` first and legacy `stable.json` last.

- [ ] Do not use `--clobber`; every GitHub and R2 immutable is globally
  preflighted. An existing byte-identical object is skipped; any difference
  stops the promotion before the first write. R2 reads treat only HTTP 404 as
  absence; redirects, authorization/rate-limit/server errors, malformed status,
  timeouts, and transport failures stop the run.
- [ ] R2 immutables are re-read immediately before each put and every put is
  read back. Neither R2 transport (S3 API or wrangler) exposes an atomic
  conditional put in this flow, so a residual race remains between the final
  404 and the put. Never claim an atomic no-clobber guarantee; investigate any
  readback mismatch immediately.
- [ ] On any failure before the stable phase, verify that no stable pointer was
  written, correct the failure, and rerun from the same sealed stage.
- [ ] If interruption occurs during the final stable-pointer loop, rerun from
  the same sealed stage. R2 has no multi-key transaction; the final writes are
  read back, idempotent, and must all converge on the already verified
  candidates. A rerun skips completed pointers and repairs the rest.

## 5. Post-promotion read-only checks

- [ ] Run or observe `r2-manifest-health.yml`; it streams fixed-size chunks,
  aborts at declared size plus one byte, and verifies exact target sets, byte
  sizes, and SHA-256 values.
- [ ] Confirm the stable version is identical for all three binaries.
- [ ] Retain the complete sealed stage, including `rollback-pointers/state.json`
  and its retained pointer bytes, plus the source SHA, workflow run URL, stage
  artifact digest, authorization, and health-check result in the release record.
- [ ] Record and protect a separate digest for the complete rollback snapshot:

  ```bash
  (cd /absolute/path/to/client-release-stage-1.21.15-<40-hex-source-sha> && \
    find rollback-pointers -type f -print0 | LC_ALL=C sort -z | \
    xargs -0 sha256sum | sha256sum)
  ```

  The promotion creates this snapshot after the workflow artifact was
  downloaded, so the previously recorded stage-artifact digest does not cover
  it. Store the snapshot digest and snapshot together in the protected release
  record before relying on rollback.

Rollback requires a new, explicit authorization and changes pointers only. It
does not rewrite any immutable archive or versioned manifest:

```bash
scripts/rollback-release-pointers.sh \
  1.21.15 \
  /absolute/path/to/client-release-stage-1.21.15-<40-hex-source-sha> \
  <40-hex-source-sha> \
  '<correlation-id>' \
  --authorized-pointers-only
```

The tool revalidates the sealed stage and retained snapshot, then globally
classifies all six live pointers before its first mutation. Every legacy
pointer must still equal this stage's v1 candidate or its retained
pre-promotion state; every strict pointer must equal this stage's v2 candidate
or already be absent. Foreign or newer pointer bytes abort the entire rollback
with no mutation. The tool repeats the same checks immediately before each
change, restores legacy `stable.json` bytes first, verifies every restore, then
deletes `stable-v2.json` and verifies HTTP 404 so new clients use their GitHub
fallback. It handles originally absent pointers and is idempotent after full or
partial forward promotion; rerun the same command after a partial rollback.
R2 has no multi-key transaction, so both forward and rollback have observable
intermediate pointer states. The chosen rollback order stays health-valid:
below 1.21.15, live legacy target/path quirks and a missing strict pointer are
accepted; at or above 1.21.15, complete exact legacy/v2 parity is mandatory.

Linux and Windows archives are byte-reproducible for identical inputs.
Timestamped/notarized macOS binaries and their archives are deterministic in
layout, target correlation, and validation, but Apple signing timestamps mean
independent rebuilds are not promised to be byte-identical. Rehearse the thin
x86_64 lane on `macos-15-intel`; the arm64 signing lane must provision and
prove Rosetta before executing every post-sign x86_64 binary.
