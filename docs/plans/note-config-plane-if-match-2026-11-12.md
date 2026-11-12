# Research, Design, Verification and Validation: the config plane's write contract (#459)

**Issue**: terraphim-clients#459
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (explicit defect, explicit acceptance criteria, local change)

## Research

### The server's contract

The server-side change is the security issue
[terraphim/terraphim-ai#3543](https://git.terraphim.cloud/terraphim/terraphim-ai/issues/3543):
"terraphim_server /config is unauthenticated with CORS Any, and API keys are
serialised to the API and storage". Its *Required* section ends with:

> Validate, persist, then swap; return an error when persistence fails; add
> `ETag`/`If-Match`.

and its *Acceptance* section:

> `POST /config` without the key returns 401; a cross-origin browser request is
> refused.

So the config plane now has two contracts the client has to honour:

| Contract | Client obligation |
|----------|-------------------|
| The config endpoints are authenticated | Send `Authorization: Bearer <key>` when a key is configured |
| Writes are protected by an ETag | `GET /config` before the write, send the `ETag` as `If-Match` |

### What the client did (before this change)

Read from `crates/terraphim_agent/src/client.rs`:

- `ApiClient::new(base_url)` built a `reqwest::Client` with a timeout and a
  user agent - **no** `Authorization` header, and no way to configure one.
- `get_config()` did a plain `GET /config` and parsed the body. The `ETag`
  response header was **discarded**.
- `update_selected_role(role)` and `post_config(cfg)` did a plain `POST` with
  no `If-Match` header.

Against the new server that means: role writes are rejected (428), and a
configuration change made elsewhere is silently overwritten instead of being
reported.

### Where the writes happen

| Call site | Write |
|-----------|-------|
| `tui_backend.rs::switch_role` (the TUI's role switch) | `update_selected_role` |
| `server_command.rs::RolesSub::Select` (`terraphim-agent roles select`) | `update_selected_role` |
| `server_command.rs::ConfigSub::Set` (`terraphim-agent config set`) | `post_config` |

All three read the configuration first, which is where the ETag comes from.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | The ETag belongs to the configuration the write is based on | The read and the write are one flow: the snapshot carries both |
| C2 | A stale ETag must not be retried behind the user's back | The conflict is reported; the write is not repeated |
| C3 | The key is optional (loopback developer mode) | The key comes from the environment and is only sent when set |
| C4 | The server redacts secrets as `[REDACTED]` | The configuration is sent back exactly as it was received |
| C5 | A server that demands `If-Match` but sends no ETag | That mismatch is reported as such, not as a generic failure |

## Design

**`crates/terraphim_agent/src/client.rs`**

1. `ApiClient::new` reads `TERRAPHIM_SERVER_API_KEY`; `ApiClient::with_api_key`
   sets the key explicitly. The key is sent as `Authorization: Bearer <key>` on
   every request (the client is a `reqwest::Client` default header, so no call
   site can forget it).

2. `ConfigSnapshot { config, etag }` and `get_config_snapshot()` - the read
   that yields both the configuration and the `ETag` that identifies it.

3. `update_selected_role(role, etag)` and `post_config(cfg, etag)` carry the
   ETag as `If-Match`. `switch_role(role)` is the convenience shape every
   caller wants: read the configuration, then write with its ETag.

4. `send_config_write` interprets the answer:

   - **412** - the configuration changed elsewhere. The client fetches the
     configuration once more (so the report can say what it looks like now) and
     fails with *"the configuration changed elsewhere ... the write was not
     applied"*. The write is **not** retried: the change was computed against a
     configuration that no longer exists, so replaying it would apply a stale
     change.
   - **428** - the server demands `If-Match` but did not return an ETag. That
     is reported as a client/server mismatch, not as a generic write failure.
   - anything else - the response body is parsed as before.

**Call sites** now read the configuration once and write with its ETag:
`tui_backend::switch_role` (through `switch_role`), `RolesSub::Select` and
`ConfigSub::Set` (through `get_config_snapshot`).

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_agent --all-targets --no-fail-fast` | 1224 passed, 0 failed |
| `cargo test -p terraphim_agent --test config_plane_client` | 6 passed |
| `cargo clippy -p terraphim_agent --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `cargo check -p terraphim_agent --features server --all-targets` | clean |
| `cargo test -p terraphim_agent --test ci_guards` | 25 passed (the workflow edits do not break the workflow guards) |
| YAML parse of both workflows | ok |

New tests (`tests/config_plane_client.rs`) drive the **real client** against a
mock server, so the assertions are on the bytes that go over the wire:

- a write carries the ETag of the configuration it is based on (`If-Match`);
- the configured key is sent as `Bearer <key>`;
- a stale ETag is reported as a conflict, the write is **not** retried, and the
  configuration is fetched again to report it;
- a server that answers 428 is reported as a missing precondition;
- the values the server redacted are sent back unchanged (`"[REDACTED]"` is
  still `"[REDACTED]"` in the request body);
- the TUI's role switch goes through the same read-then-write path and carries
  the ETag.

### CI

The server-mode client lives behind the `server` feature. The test build
enables it through the `repl-full` dev-dependency, but a plain build does not -
so a break in the client would only be found by whoever builds with the
feature on. Both workflows now compile it explicitly:

```yaml
- run: cargo check -p terraphim_agent --features server --all-targets
```

### A flaky test found on the way

`error_handling_test.rs::test_network_timeout_handling` asserts that the error
of a request to an unreachable address mentions a network problem. `reqwest`
reports *every* request failure as "error sending request for url (...)" and
puts the cause in the source chain, so the assertion only held when the
connection hung (the usual case). When the network gave up quickly - which
happened once during a full-suite run - the test failed although nothing was
wrong. The assertion now also accepts "sending request", exactly like the
sibling test a few lines below.

## Validation

| Acceptance criterion (issue #459) | Result |
|---|---|
| "Read `TERRAPHIM_SERVER_API_KEY` (or a CLI option) and send it as `Authorization: Bearer` when set" | PASS - the key is read from the environment (or set explicitly) and sent as a bearer token on every request |
| "`GET /config` before each write and send its `ETag` as `If-Match`" | PASS - `get_config_snapshot` + `send_config_write`; the TUI, `roles select` and `config set` all read first |
| "On 412, re-fetch once and report 'configuration changed elsewhere' instead of retrying the write silently" | PASS - the configuration is fetched again, the conflict is reported, and the test asserts the write was sent exactly once |
| "Send `"[REDACTED]"` values back unchanged" | PASS - the configuration is written back exactly as received; the test asserts the marker survives the round trip |
| "role switching works in loopback mode and with a key" | PASS - no key means no `Authorization` header (loopback developer mode); a key means a bearer token |
| "a stale ETag surfaces the 412 message" | PASS - the error names the conflict and the current selected role |

## Limits

- The tests use a mock server, not a real `terraphim_server` binary: the
  client's side of the contract is proven, but the server's side (its own
  `If-Match` handling) is not exercised here.
- The conflict is reported, not resolved. The client does not rebase the
  change onto the newer configuration - a rebase would have to merge the
  change into whatever the server holds now, which is a policy question
  (last writer wins? merge? refuse?) rather than a client detail.
- The key is read from the environment only. A command-line option would leak
  the key into the process list, so it was not added.
- The bearer token is sent on **every** request to the server, not only on the
  config endpoints. That is what a global authentication layer on the server
  expects; if the server ever authenticates the config endpoints differently
  from the rest, this has to be split.
