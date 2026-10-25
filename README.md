# Baton — Product and Engineering Specification

**Status:** M0–M3 libraries and the M4 experimental local CLI/daemon are implemented,
with experimental M5 managed Codex initialization, bounded lifecycle metadata and
cancel-only approvals, turn interruption and callback refusals added on 25 October 2026.
Real source-work capture and agent transfer remain in progress. Native three-OS validation
is pending; experimental engines and the executable remain disabled by default.

**One-line product statement:** Baton lets a developer stop an AI coding task in one agent and continue it in another agent or on another computer from a verifiable, recoverable execution checkpoint—not merely a written handoff.

## Start here

1. Copy **[CODEX_START_PROMPT.md](CODEX_START_PROMPT.md)** into a Codex session after placing this entire documentation folder at the new repository root.
2. Read **[PRD.md](PRD.md)** for the product, users, release boundaries, requirements, and acceptance criteria.
3. Read **[AGENTS.md](AGENTS.md)** for binding instructions to Codex and other implementation agents.
4. Read **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** and **[docs/PROTOCOL.md](docs/PROTOCOL.md)** before implementing persistence or process control.
5. Read **[docs/INTEGRATIONS.md](docs/INTEGRATIONS.md)** and **[docs/INSTALLATION.md](docs/INSTALLATION.md)** for integration and distribution choices.
6. Read **[docs/SECURITY.md](docs/SECURITY.md)** and **[docs/TESTING.md](docs/TESTING.md)** before declaring any feature complete.
7. Use **[docs/ROADMAP.md](docs/ROADMAP.md)** to implement in testable increments; record deviations in **[docs/DECISIONS.md](docs/DECISIONS.md)**.
8. Consult **[docs/REFERENCES.md](docs/REFERENCES.md)** for official vendor documentation and revalidate version-sensitive details.

## Preferred installation strategy

**Baton is a native local developer tool first.** It ships as a signed native CLI and local daemon. A built-in *local MCP server* exposes controlled tools to agents. Codex/Claude plugins and skills are optional packaging and onboarding layers; they are not the engine or the security boundary. A hosted synchronization service is a later stage.

- User-facing proposed commands: `baton init`, `baton doctor`, `baton start`, `baton status`, `baton checkpoint`, `baton transfer`, `baton accept`, `baton recover`, `baton mcp serve`, `baton uninstall`.
- Transfer, agent start, MCP and installation commands remain planned. The
  experimental CLI currently supports repository/task metadata, status/events,
  forensic capture and isolated restoration.
- The product should require explicit trust and consent for repository access, process supervision, instrumentation, and any cross-machine upload.

## First demonstrable milestone

On **one machine**, capture unfinished Codex work at a verified safe boundary; restore a fresh Git worktree; allow Claude Code to resume with evidence of the last failed test; block stale ownership; survive injected crashes. Only call it successful if the tests in `docs/TESTING.md` pass.

## Product principle

**Continue the work, not the conversation.** The observable file and operation state is authoritative. Agent-generated explanations are helpful evidence, never the sole basis of correctness.

## Developing the M0 baseline

Install Rust stable (including rustfmt and Clippy) and Git, then run:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --all-targets --locked
cargo test --workspace --locked
```

Windows requires either Visual Studio C++ Build Tools for MSVC or a configured
GNU toolchain/linker. On the current development host MSVC `link.exe` is absent;
the checks pass using `cargo +stable-x86_64-pc-windows-gnu` in place of `cargo`.
CI uses native Windows, macOS, and Linux runners. A configured workflow is not
evidence of a completed CI run.

To build and test the experimental engines explicitly:

```text
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --locked
```

Use the same Windows GNU override where needed. M1 implements transactional
metadata reservations. M2 adds local forensic capture and fresh-worktree restore;
all continuation outcomes remain BLOCKED. M3 adds native process control and an
audited effect ledger through trusted embedding APIs, tested with synthetic agents.
It grants no transfer activation and provides no filesystem/network sandbox.
Keep databases on a private local filesystem outside repositories.
M4 adds owner-checked local state, OS identity recovery and user-scoped IPC.
New stores use database schema 15; schema-1 through schema-14 stores refuse without rewriting.
Transfer intent, preflight reports and aborts are durable and auditable. They do
not stop agents, commit ownership or activate receivers; preflight remains BLOCKED.
Artifact restore persists intent before creating a worktree. A retry can reuse
only its bound completion receipt after rechecking all receiver content; partial
or unrecognized targets are preserved and refused.
Native process scopes and observed empty-job drains now have separate immutable
records tied to live control objects. These facts do not establish a complete
safe transfer boundary or enable receiver activation.
Receiver staging, ownership commit and recovery now have durable protocol paths.
Production still cannot enter PREPARED with the missing source proofs; activation
is explicitly disabled. Closed fixture tests exercise the storage path with
test-only boundary assumptions, not real vendor or sandbox validation.
See [M1 evidence](docs/M1-IMPLEMENTATION.md), [M2 evidence](docs/M2-IMPLEMENTATION.md)
and [M3 behavior and limits](docs/M3-IMPLEMENTATION.md).

Build the experimental native executable with
`cargo build -p baton-cli --features experimental --locked` (use the Windows GNU
override where needed). Initialization and daemon startup are separate opt-ins:

The built executable is `target/debug/baton` (`target/debug/baton.exe` on Windows).
Use that path in place of `baton` below unless it is on your PATH.

```text
baton --state <new-private-state-directory> init --repo <repository>
baton --state <state-directory> daemon
baton --state <state-directory> --json status
```

Run the daemon in a separate terminal and use `baton --help` for available
commands. No global configuration or service is installed. `--operation-id`
allows exact mutation retries; `--epoch` binds ownership-sensitive requests.
Task/checkpoint/restore IDs default to the operation ID so a lost response does
not require inventing a new operation. All continuation reports remain BLOCKED.
See [M4 progress and limits](docs/M4-IMPLEMENTATION.md).

The Windows simulated M4 tracer now exercises successive handoffs, retaining
receiver edits in a fresh worktree at the next epoch. This uses private test-only
boundary assumptions, not real Codex/Claude sessions. Tool admission is a fresh
native-host permit distinct from observed tool events; neither saved requests nor
lease expiry authorize replay. Production transfer remains BLOCKED. See ADR-027.

M5 adds an opt-in Rust App Server client. Its actual Codex 0.164.0 handshake has
been tested with a private temporary home, alongside malformed/oversized reply,
EOF, timeout and cancellation tests. It does not start a model turn or grant
continuation authority. See [M5 plan and evidence](docs/M5-IMPLEMENTATION.md).
M5 now also records bounded turn and command/file-change lifecycle metadata with
durable vendor IDs, idempotent retries and explicit gaps. Tool success remains
uncertain without independent effect policy; raw command/output content is not
captured by this slice. Real source-work and cross-agent continuation remain open.

See [the M0 implementation and evidence report](docs/M0-BASELINE.md) for the
module boundaries, schema/error policy, traceability, and remaining exit gate.
