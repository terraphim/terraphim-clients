# Research, Design, Verification and Validation: the PreToolUse hook fails closed (#465)

**Issue**: terraphim-clients#465
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (explicit defect, explicit acceptance criteria, local change)

## Research

### The defect

The guard decides whether a command is destructive; the hook decides what the
agent's caller is allowed to do. The issue's evidence:

| Command | Guard | Hook | Expected |
|---|---|---|---|
| `git reset --hard` (control) | block | deny | deny |
| `git reset --hard; echo $'é'` | rc 101, no output | **no deny** | deny |
| `rm -rf /home/u/p $'é'` | rc 101, no output | **no deny** | deny |

A guard that produces **no output at all** turns into an *allow*. The issue
names the reason this matters independently of the guard's own correctness:

> Any future crash, abort, OOM kill, signal, or startup failure of the guard
> becomes a silent bypass. This is a single point of failure in the last line
> of defence.

And it rules out the obvious in-process fix:

> A stack overflow is an abort, not a panic, so a `catch_unwind` fail-closed
> boundary inside the guard cannot catch it. The abort still produces no
> output, and the hook still allows.

### What the code did (before this change)

`crates/terraphim_agent/src/main.rs`, the `PreToolUse` arm of the hook:

```rust
let guard = guard_patterns::CommandGuard::new();
let guard_result = guard.check(command);
if guard_result.decision == guard_patterns::GuardDecision::Block { /* deny */ }
```

The guard ran **in the hook's own process**. A crash, an abort or an OOM kill
inside it therefore killed the hook as well: no envelope, no decision, and the
caller - the shell hook, which allows anything it does not understand - let the
command through.

### Where the fix belongs

The issue is explicit about the layering:

> Keep the existing `--fail-open` behaviour of the guard itself for other
> callers if it is needed there; the fail-closed behaviour belongs at the hook,
> which is the component that makes the final decision.

So the `guard` command keeps its behaviour, and the **hook** becomes the
fail-closed component.

### The other half of the issue (not in this repository)

The installed shell hook (`~/.claude/hooks/pre_tool_use.sh`) is the other
decision-maker, and it has the same shape:

```bash
GUARD_RESULT=$($AGENT guard --json <<< "$COMMAND" 2>/dev/null || echo '{"decision":"allow"}')
```

The `|| echo '{"decision":"allow"}'` turns a guard that failed into an allow.
That script lives in the dotfiles/`terraphim-skills` tree, not in this
repository, so it is **not** part of this change - but it is the same defect
and is called out in the issue comment.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | An abort cannot be caught in-process | The guard runs in a child process |
| C2 | The hook is the final decision-maker | "No decision" means *block*, not allow |
| C3 | The `guard` command keeps its behaviour | Only the hook changes |
| C4 | The decision must name the failure | The deny reason says what happened and with which exit status |
| C5 | The hook must not be bypassable by accident | The guard program is only overridable through a hidden option the installed hook never passes |

## Design

**`crates/terraphim_agent/src/hook_guard.rs`** (new):

```rust
pub enum GuardOutcome {
    Allow,
    Sandbox { reason: String },
    Block { reason: String },
    Failed { message: String },
}

pub fn parse_guard_output(stdout: &str, status: Option<i32>) -> GuardOutcome;
pub async fn run_guard(program: &Path, command: &str) -> GuardOutcome;
```

- `run_guard` runs `<program> guard --json` with the command on the child's
  standard input, and reads the decision from its standard output. The child's
  standard error is discarded: a failing guard must not write into the hook's
  output.
- `parse_guard_output` is the rule: empty output, output that is not JSON, and
  JSON without a recognised decision **all** mean "the guard did not decide",
  and produce `Failed` - which blocks.
- The exit status is only used to describe the failure ("exit 101", or "the
  guard was killed" when there is no status).

**`crates/terraphim_agent/src/main.rs`**, the `PreToolUse` arm:

- The guard now runs through `run_guard`, using this binary (`current_exe`)
  unless a program was named explicitly.
- `Block` denies with the guard's reason, exactly as before.
- `Failed` denies with `guard failed to produce a decision (<what happened>);
  blocking by default`.
- `Allow` and `Sandbox` let the call through, as before.
- If the guard program cannot even be located, the hook fails with an error
  instead of allowing the call.

**`--guard-program <PATH>`** (hidden) names the program the hook asks. It
exists so the fail-closed behaviour can be tested with a stub, and for
wrappers; the installed hook never passes it, so it is not a way to bypass the
guard in the normal path.

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_agent --all-targets --no-fail-fast` | 1229 passed, 0 failed |
| `cargo test -p terraphim_agent --test hook_guard_fail_closed` | 6 passed |
| `cargo test -p terraphim_agent --test hook_safety` | 5 passed (the existing hook tests still pass with the guard out of process) |
| `cargo clippy -p terraphim_agent --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

New tests (`tests/hook_guard_fail_closed.rs`) drive the **real hook binary**
with a stub guard:

- a guard that exits 101 without output -> the hook **denies**, and the reason
  names the failure and the exit status;
- a guard that is killed (`kill -9`) -> the hook denies;
- a guard that prints something that is not a decision -> the hook denies;
- a guard that cannot be started -> the hook denies;
- a guard that blocks -> the hook denies with the guard's reason;
- a guard that allows -> the call goes through.

The unit tests in `hook_guard.rs` cover the parsing rule directly (empty
output, unparseable output, an unrecognised decision, a killed guard).

## Validation

| Acceptance criterion (issue #465) | Result |
|---|---|
| "The hook should deny when the guard produces no parseable decision" | PASS - the stub tests cover no output, rubbish output, a killed guard and a guard that cannot be started |
| "deny the tool call with a message that names the guard failure" | PASS - the reason names the failure and the exit status |
| "Keep the existing `--fail-open` behaviour of the guard itself" | PASS - the `guard` command is unchanged |
| "The in-process `terraphim-agent hook --hook-type pre-tool-use` entry point ... needs the same treatment" | PASS - the in-process hook now asks the guard out of process and blocks when it gets no decision |
| "A test that runs the hook with a guard stub that exits non-zero and produces no output, and asserts the hook denies" | PASS |
| "the existing hook tests for the recognised-decision paths, which must still pass" | PASS - `hook_safety` is 5 passed / 0 failed |

## Limits

- The hook now starts a process per Bash tool call. The guard command does not
  initialise the TUI service, so the cost is the process start itself (a few
  milliseconds), not a service start.
- The *shell* hook (`~/.claude/hooks/pre_tool_use.sh`) has the same defect -
  `|| echo '{"decision":"allow"}'` - and lives outside this repository. Until
  it is fixed there, a crash of the guard still reaches the shell hook as "no
  output". This change makes the agent's own hook fail closed; the shell hook
  needs the matching one-line change in its own tree.
- The guard program can be overridden with `--guard-program`. The installed
  hook never passes it, so it is not a bypass of the normal path - but anyone
  who can run the hook command directly can choose its guard.
