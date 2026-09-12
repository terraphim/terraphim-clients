# Memory benchmark fixture

Step 1 of the judge-free memory measurement plan for `terraphim-agent memory`
(terraphim/terraphim-clients#255, issue #259). The fixture is the corpus and
ground truth that `memory_bench::evaluate` (step 2) runs through the unchanged
`memory_retrieve::retrieve`. It is committed so the benchmark is reproducible in
CI and small enough to be read line by line.

## Files

| File | Records | Shape |
|------|---------|-------|
| `corpus.jsonl` | 60 | one `terraphim_agent_evolution::MemoryItem` per line, serde JSON |
| `queries.jsonl` | 50 | one `{"query": "...", "expected_ids": ["..."]}` per line |

corpus.jsonl SHA-256: ea9057b2a807adf8d7602a6dc13104d83bbfff5c94eca45036745730d699214e

`tests/memory_fixture_integrity.rs` asserts that hash, that every corpus line
parses as `MemoryItem`, that ids are unique, that every `expected_id` exists,
and that no unredacted host, path or credential shape remains.

## Provenance

Built on 2026-09-12 from the private learnings directory captured by
`terraphim-agent learn` hooks (1,032 markdown files: 1,029 `learning-*.md`,
3 `correction-*.md`). The source directory is not in any repository. Nothing
in the fixture was written by hand; the build is:

```
scripts/build_memory_fixture.sh [learnings_dir] [out_dir]
```

which runs `crates/terraphim_agent/examples/build_memory_fixture.rs`
(`cargo run -p terraphim_agent --example build_memory_fixture`) and prints the
SHA-256 above. Two consecutive builds produce byte-identical files.

## Selection (mechanical)

1. Every `correction-*.md` file becomes one item of type `LessonLearned`.
   Its `## Original` text is a query whose expected id is that correction.
2. Every `learning-*.md` file is parsed with the capture module's
   `CapturedLearning::from_markdown`. The full command is read from the
   `## Command` body section because the front matter parser keeps only the
   first line of a multi-line command. Learnings whose working directory,
   command or error output refer to the `zestic-ai/` client tree are left out
   by that path prefix (120 of 1,029).
3. Learnings are grouped by their redacted, whitespace-normalised command. A
   command captured more than once is a repeated-failure cluster (57 clusters
   from 909 learnings). The earliest capture of each cluster, by capture time
   then id, becomes the corpus item of type `Experience`; `access_count`
   records the cluster size. The command is a query whose expected id is that
   earliest capture.
4. Items are ordered by `created_at` then id. `created_at` is derived from the
   millisecond suffix of the capture id, which is the capture time, because
   the front matter parser substitutes the current time when a multi-line
   command contains `---`.
5. Queries are the 3 corrections plus the 47 largest clusters (size
   descending, then earliest capture), ordered by expected id. Clusters beyond
   the 50-query cap stay in the corpus as distractors without a query.

Caps: at most 200 items (60 used), 20 to 50 queries (50 used). Error output in
`content` is cut at 2,000 characters with a `[truncated]` marker (18 items).
Every item has `importance: Medium`, `last_accessed: null` and a single
association `origin: learning|correction`, matching what `memory capture`
writes today.

## Redaction

Every text field passes through, in order:

1. ANSI escape sequences removed.
2. `scheme://user:password@` credentials in URLs, then `user@host` pairs and
   e-mail addresses, become `[USER]@[HOST]`.
3. Syslog and journal host fields (`Apr 22 21:22:16 <host> proc[pid]:`) become
   `[HOST]`.
4. IPv4 addresses become `[IP]`; `127.0.0.1` and `0.0.0.0` are kept.
5. `ssh` and `scp` targets after their options become `[HOST]`.
6. Fully qualified host names whose top-level domain is one of `cloud`, `ai`,
   `com`, `io`, `net`, `org`, `dev`, `engineer`, `local`, `lan`, `internal`,
   `localhost` become `[HOST]`, except a short allowlist of public developer
   domains (`github.com`, `crates.io`, `docs.rs`, `rust-lang.org`,
   `cloudflare.com`, `npmjs.com`, `pypi.org`, `docker.io`, `ghcr.io` and a few
   others listed in the example). Source-file extensions are not treated as
   domains.
7. 1Password references become `op://[REDACTED]`; `worker:`, `host:` and
   `hostname:` labels lose their value; `Bearer <token>` and any
   `token|secret|password|passwd|api_key` followed by a value of eight or more
   characters lose the value; runs of 32 or more hexadecimal characters become
   `[HEX_REDACTED]`.
8. `/Users/<name>` and `/home/<name>` become `/Users/[USER]` and
   `/home/[USER]`; `zestic-ai/<dir>` becomes `zestic-ai/[CLIENT]`.
9. Finally the capture module's own `terraphim_agent::learnings::redact_secrets`
   (AWS, OpenAI, Slack and GitHub key shapes, connection strings, and
   `TOKEN=`, `PASSWORD=`, `API_KEY=` style environment assignments).

The rules are structural on purpose. The build script, the example and the
integrity test contain no list of the host names, user names, vault names or
client names they remove, because they are committed to a repository that is
mirrored publicly.

Ids are `<uuid-hex>-<unix-millis>` values generated by the capture hook and
are not redaction targets.

## Known noise and over-redaction

These are left as the mechanical rules produced them; filtering them would be
a hand judgement of relevance.

* `[AWS_SECRET_REDACTED]` appears where the capture pipeline's 40-character
  pattern matched long path segments such as `/opt/homebrew/.../lib/python3`
  at capture time. That text is already redacted in the source files and is
  not recoverable here.
* Six queries are test artefacts or fragments of chained commands split at
  capture time: `fake-cmd`, `nonexistent-final-learning-test`,
  `prove-test-claude-hook-direct` (also the third correction),
  `print('OK')"`, `print('YAML valid')"`, and the `cd ...` prefixes of longer
  command chains.
* Several `cd <dir>` commands carry the error output of the command that
  followed them in the chain, because the hook records the first failing
  segment.
* `git push` is the largest cluster (41 captures) and its error output is the
  single word `rejected`.
* `[USER]@[HOST]` also replaced two non-address shapes: a systemd unit
  `postgresql@14-main.service` and an `@adf:` mention preceded by `\n`.

## Thesaurus used

No thesaurus is consumed at build time: selection and ground truth are
mechanical and do not rank anything. The reference thesaurus for retrieval
(proposed: the Terraphim Engineer role, pinned by hash) is an open decision on
#255 and is recorded by step 2 when `memory_bench::evaluate` first runs.

## Rebuilding

Rerun `scripts/build_memory_fixture.sh`, replace the SHA-256 line above with
the printed value, and read every changed line before committing.
