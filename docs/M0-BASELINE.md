# M0 implementation and evidence

Date: 22 October 2026. Status: implemented locally; **M0 exit gate pending**.
No functional FR or transfer invariant is claimed complete by this baseline.

## Plan and implemented boundaries

1. Establish a Cargo workspace with `baton-protocol`: strict diagnostic schema,
   canonical errors, and explicit recovery labels (FR-13/19, INV-08/09 support).
   Verify M0-001–003 before adding runtime behavior.
2. Establish `baton-test-support`, a test-only Git fixture helper in owned
   temporary directories (FR-01/09, INV-05 harness support). Verify M0-004–007
   with real Git, no agents, network, or existing project access.
3. Configure stable Rust format/lint/build/test CI for three native OS families;
   document traceability, schema/security/portability decisions, and actual local
   evidence. Do not implement M1.

There are no callers, runtime config, public command exports, fixtures containing
user data, or binaries to preserve. Public Rust exports are in each crate's
`src/lib.rs`; integration tests are their current consumers. Later store and
daemon modules depend on protocol; test-support must remain a dev-dependency of
future production crates. Dependencies are Serde, test JSON serialization, and
tempfile; no unused runtime stack is introduced. See ADR-016.

## Versioned diagnostics

The JSON diagnostic is `{"schema_version":"0.1.0","code":"INVALID_INPUT"}`.
Both fields are required. Unknown fields, codes, and schema versions reject
during decoding. Decoding errors must not be treated as valid diagnostics or
permission grants. No migration, persistence, or public IPC interface exists yet.
Human formatting is `CODE: reason. corrective action.`; `ErrorCode::reason()`
and `action()` supply safe static text. JSON clients look up guidance by code.

| Code | Meaning | Corrective action |
|---|---|---|
| INVALID_INPUT | Contract violation | Correct input |
| UNSUPPORTED_SCHEMA | Unsupported schema | Compatible build or explicit migrator |
| IO_ERROR | Required local I/O failed | Inspect permissions/storage |
| GIT_UNAVAILABLE | Git executable not found | Install Git / correct PATH |
| GIT_FAILED | Git exited unsuccessfully | Inspect repository/command preconditions |

These errors are deliberately generic; raw stderr, paths, and secrets are never
embedded. More specific persistence/authority/process codes belong to their
milestones. `VERIFIED`, `DEGRADED`, and `BLOCKED` are distinct wire labels only;
M0 cannot compute any recovery outcome and has no default success state.

## Traceability

| Test | Requirement / invariant association | Evidence established |
|---|---|---|
| M0-001 | FR-19, NFR-08 | Reject missing/unknown versions, fields, codes |
| M0-002 | FR-19 | Stable diagnostic encoding, roundtrip, human guidance |
| M0-003 | FR-13, INV-08/09 | Three distinct strict labels, no success alias |
| M0-004 | FR-01/09, INV-05 | Independent base/index/working bytes, untracked binary, CRLF |
| M0-005 | FR-01/09, INV-05 | Independent fixture roots, Unicode/spaces, sentinel preserved |
| M0-006 | FR-19 | Failed Git command is an error with no raw path disclosure |
| M0-007 | FR-01, INV-05 | Inherited Git directory/worktree/config cannot redirect fixtures |

Test functions contain the corresponding identifiers. T-001–T-010 and the crash
matrix are **not run or implemented**: they require future ownership, storage,
supervision, and transfer code. M0 has no durable transition to fault-inject.

## Test evidence

- Host: Windows NT 10.0.26300.0, x86_64.
- Rust: `rustc 1.98.0 (88d9e12ae 2026-08-18)`, Windows GNU;
  Cargo `1.98.0 (797e8a9bc 2026-08-05)`.
- Git: `2.51.0.windows.1`. Agents: none; no mocked or live vendor API calls.
- Revision: unavailable; supplied directory has no `.git` metadata. Cargo.lock
  fixes resolved dependencies; no commit or remote publication was performed.
- Initial red run: tests could not compile because the planned protocol types and
  GitFixture did not exist. After implementation, seven integration tests pass.
- M0-007 mutation check: disabling inherited `GIT_*` removal makes its child
  observe `core.autocrlf=true` instead of `false`; the test fails. Removal restored.
- `cargo +stable-x86_64-pc-windows-gnu fmt --all -- --check`: passed.
- `cargo +stable-x86_64-pc-windows-gnu clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo +stable-x86_64-pc-windows-gnu build --workspace --all-targets --locked`: passed.
- `cargo +stable-x86_64-pc-windows-gnu test --workspace --locked`: **7 passed,
  0 failed, 0 ignored**; no additional unit/doc tests.
- The first plain `cargo test --workspace` selected Windows MSVC and failed
  before tests: `linker link.exe not found`. No VS C++ tool installation was
  discovered. GNU was used explicitly without changing host defaults.
- Native macOS, Linux, and Windows MSVC CI: configured, **not executed** here.
  No remote repository is configured; CI logs cannot yet be supplied.
- No benchmark or performance claim; performance targets belong to later work.

## Security and portability review

The core is safe Rust with workspace `unsafe_code = forbid`. There is no listener,
daemon autostart, cloud client, capture, enrollment, epoch grant, shell replay,
or restore path. The fixture helper invokes Git directly with argument arrays,
never through a shell. It creates fresh temp directories only, ignores inherited
`GIT_*` settings, disables system/global Git config and signing, uses an empty
template/hook directory, and fixes commit timestamps. It neither changes CWD nor
mutates process-wide environment; M0-007 uses a subprocess for hostile inputs.
TempDir owns cleanup of synthetic data only. The helper is trusted test code,
not a sandbox or a production Git service; it accepts arbitrary trusted Git args.

There are no Cargo OS feature switches: platform selection uses target `cfg` for
the null configuration device (`NUL` versus `/dev/null`). Native CI covers GNU/Linux,
macOS, and Windows MSVC. Fixture paths include spaces/Unicode and content tests
disable autocrlf conversion explicitly. Non-UTF-8 template paths fail with
INVALID_INPUT; lossless non-UTF-8 production paths need separate M2 design/tests.
Symlinks, junctions, hardlinks, permissions, long/case-sensitive paths, Job Objects,
process groups, named pipes, SQLite durability, and CAS fsync semantics remain
unimplemented and unclaimed. This is a baseline, not an experimental transfer
feature that can be enabled.

## File inventory and next step

Created: `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.gitignore`,
`.github/workflows/ci.yml`; each of the two crates has `Cargo.toml`, `src/lib.rs`,
and one integration test file (`contracts.rs` or `fixtures.rs`); this report.
Updated: README, ROADMAP, TESTING, DECISIONS. Original product and safety
requirements remain intact. No architectural safety deviation or waived gate.

Recommendation: run the configured CI in a Git host and resolve any native OS
failures before closing M0. The exact next milestone is **M1 — Durable tasks,
epochs and event log** (FR-01/02/04/07/08; INV-01/07; T-001/002/008). Do not start
it before the M0 three-OS exit gate is satisfied.

Subsequent sequencing decision: the user explicitly authorized the next part
after this report. ADR-017 permits experimental M1 development while retaining
the pending M0 native validation gate. The original evidence above is historical;
it does not imply later tests or milestones have passed.
