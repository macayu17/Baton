# M1 implementation plan and evidence

Status: metadata slice implemented and tested locally; experimental library,
disabled by default. Native portability and production authority gates remain open.

Historical scope note: this report describes the M1 increment and database schema
1. M5 now creates schema 19 and refuses older databases without auto-migration;
see `M4-IMPLEMENTATION.md` and ADR-020. The M1 test counts below are historical.

The user authorized proceeding after the M0 report on 22 October 2026. M0's
three-OS validation remains pending; this authorizes development, not a waived
release gate. See ADR-017 for the bounded sequencing and authority decisions.

## Plan

1. Add strict versioned task/request/event types and canonical storage/authority
   diagnostics in `baton-protocol` (FR-02/04/07/08; INV-01/07).
2. Add `baton-store`: SQLite WAL/FULL, transactional enrollment and tasks,
   authenticated in-process actor capabilities, task ACLs, metadata reservations,
   epoch CAS, heartbeat/SUSPECT, append-only journal/audit and idempotency ledger.
   Keep state outside Git roots. Never activate processes or transfer writers.
3. Test-first verification: M1-001 enrollment/no worktree changes; M1-002 task
   validation/permissions/schema; T-001-M1 concurrent claims and duplicate keys;
   T-002-M1 stale epoch rejection/audit after reservation cancellation/reclaim;
   T-008-M1 persistent replies/conflicting keys; M1-003 expiry/no takeover;
   M1-004 hard process kill before/after commit and retry; M1-005 journal integrity
   and unsupported database refusal. Run full workspace format, lint, build/tests
   with and without the experimental feature.

## Scope boundaries

An M1 reservation grants authority only for coordinator metadata mutations.
It is never ACTIVE and grants no filesystem/tool execution permission. Cancelling
a never-activated reservation permits another metadata claim with a higher epoch;
there is no release/takeover mechanism for actual running writers. M3/M4 must
introduce supervised execution, fencing proof, two-phase handoff, and activation.
T-001/002/008 here cover only implemented metadata paths, not their complete
future transfer/activation assertions. All automatic continuation remains absent.

The library is behind `baton-store/experimental` because native OS IPC identity,
Windows state-directory ACL validation and three-OS runtime evidence are not yet
available. Trusted embedding code bootstraps local actor capabilities; actor IDs
from serialized requests alone confer no authority. No daemon or agent-facing
endpoint is introduced.

## Implemented interface and persistence

`Store::create` creates a fresh, owned directory outside any Git repository;
existing directories are refused. `Store::open` opens only an identified Baton
database (application ID `0x4241544e`, user_version 1). It refuses unknown/future
versions and never automatically migrates. Unix creation uses directory mode 0700
and database mode 0600; Windows inherits permissions, which are not validated yet.
Symbolic/reparse roots and database/sidecar files are refused; Unix hardlinks are
also refused. State must be on a trusted local filesystem. No cloud/export exists.

Trusted embedding code calls `create_local_actor` and holds the returned opaque
capability. Its secret is hashed with BLAKE3 in SQLite, absent from Debug, and not
serializable. `execute(&Actor, &Request)` authenticates it, matches the request
actor ID, and enforces repository ownership/task ACLs. This bootstrap is not a
public endpoint or proof of an OS identity. Lost in-memory credentials cannot be
recovered automatically; secure credential persistence is a later requirement.

The strict schema-0.1.0 request carries operation ID, actor ID, expected epoch,
and action. Enrollment is pre-task and uses expected epoch 0. Creation includes
the requested opaque task ID and repository ID, also at epoch 0. Existing-task
actions always name the task and expected epoch. Heartbeat, cancellation and
reported intent additionally require the current lease ID and owner.

| Action | Result and guard |
|---|---|
| EnrollRepository | Read-only Git discovery; canonical root/common Git dir; repeat enrollment returns original repository ID to its creator |
| CreateTask | Persist unique ID, goal, scope, creator, agent metadata; initial UNCLAIMED/epoch 0 and first journal event |
| GrantAccess | Task creator grants an existing actor task access at the current epoch; does not grant filesystem authority |
| ClaimTask | Task ACL required; UNCLAIMED only; epoch CAS increases by one and creates a fresh reservation lease |
| Heartbeat | Matching reserved actor/lease/epoch; bounded duration 1–300,000 ms; cannot renew SUSPECT or expired reservations |
| CancelReservation | Matching unexpired never-activated reservation only; clears owner/lease without decreasing epoch |
| RecordIntent | Bounded agent-reported text under current reservation; never promoted to observed fact |

`task`, `events`, and `audit` authenticate and enforce the same task ACL. Read
transactions keep task cursors and journal views in one SQLite snapshot. Event
IDs, per-task sequence, actor, epoch, originating operation and coordinator time
are recorded atomically. Control events are coordinator observations; supplied
intent is explicitly AgentReported. No tool evidence is captured in M1, so
`evidence_hash` is explicitly null; there is no fabricated immutable evidence.
Inference generation is absent. Goals/scope/intent are explicit caller-supplied
local text, not redacted or encrypted; do not put credentials or private raw
transcripts into them. No repository file content is captured.

SQLite WAL, FULL synchronous mode and foreign keys are verified on every
connection. Mutations use BEGIN IMMEDIATE and checked epoch compare-and-swap.
State, journal, cursor, audit and operation reply commit together; append-only SQL
triggers reject changes/deletions of journal, audit and operation history. These
triggers are not tamper-proof against a process with direct database access.
Sequence/cursor checks detect middle gaps and missing tails. No database lease
prevents arbitrary shells from editing files.

Operation IDs are global and bind the authenticated actor plus BLAKE3 fingerprint
of the entire typed request. Logical denials are cached/audited just like
successes. Retrying the original request returns its original result even after
newer epochs; this is a historical acknowledgement, not current write authority.
Changed requests return IDEMPOTENCY_CONFLICT without replacing the original
result. I/O, busy and corruption failures do not acknowledge a new operation.
No request body, command stderr, token or arbitrary path is copied into audit or
diagnostics. Safe metadata creation/bootstrap are local lifecycle APIs, not RPCs.

Lease timestamps are sampled after obtaining the writer lock and clamped to a
persisted clock high-watermark. Every relevant new authorized request checks
expiry; once observed, SUSPECT remains sticky even after clock rollback, keeps
owner/epoch, and blocks all ownership/lease continuation. Reads and exact retries
do not renew expiry or run a scanner. GrantAccess is an administrative permission
change, not lease continuation. A future managed writer cannot use reservation
cancellation as a fencing substitute.

New safe diagnostic codes: UNAUTHORIZED, FORBIDDEN, NOT_FOUND, STALE_EPOCH,
IDEMPOTENCY_CONFLICT, BLOCKED, STORE_BUSY, STORE_FAILURE, CORRUPT_STORE. Existing
M0 codes and human formatting remain unchanged. No raw SQLite/Git errors escape.

## Traceability and test evidence

| Test | Association | Result established |
|---|---|---|
| M1-P01/P02 | FR-02/07/08/19, INV-01/07 | Strict request decoding; canonical new diagnostics |
| M1-001 | FR-01, INV-05 | Idempotent enrollment; index and worktree bytes preserved; in-repo state creation refused |
| M1-002 | FR-02/04/07, INV-01/09 | Actor mismatch/ACL/invalid duration denied; reported intent not a fact |
| T-001-M1 | FR-07, INV-01 | Sixteen independent connections/authorized actors yield exactly one claim winner |
| T-002-M1 | FR-07, INV-01 | Epoch 1 cancellation then epoch 2 claim; old heartbeat/intent/cancellation denied and audited |
| T-008-M1 (three tests) | FR-04/08, INV-07 | Persistent retry across reopen; conflicting inputs rejected; all metadata actions/denials replay identically; eight concurrent identical claims return one reply/event |
| M1-003 | FR-07, INV-06 | Expiry produces sticky SUSPECT; rollback of clock cannot enable takeover/renewal |
| M1-004 | FR-04/07/08, INV-01/07 | Real child hard-kill immediately before commit and after commit/before reply; recovery/retry yields one epoch and event |
| M1-005/009 | FR-04, INV-09 | Append-only history; middle and tail gaps block reads/mutations |
| M1-006 | FR-04/07/08, INV-01/07 | Injected event insert failure rolls back lease and operation key; same-key retry succeeds after repair |
| M1-007 | FR-04, NFR-08 | Future database rejected; no overwrite or auto-downgrade |
| M1-008 | FR-04/19, INV-07 | WAL/FULL settings confirmed; real competing SQLite write lock returns STORE_BUSY and safely retries |
| M1-010 | FR-01, INV-05 | Subdirectory/dot aliases and Windows ASCII casing identify the original repository |
| M1-011 | FR-04/07, INV-01/09 | One hundred heartbeats alongside readers preserve consistent committed cursors |

Environment: Windows NT 10.0.26300.0, x86_64-pc-windows-gnu; rustc 1.98.0
(88d9e12ae), Cargo 1.98.0, Git 2.51.0.windows.1; rusqlite 0.40.2 / bundled
SQLite 3.53.2 (locked). Synthetic temporary repositories only, no live or mocked
vendor agent. There is no Git metadata or revision ID in the supplied workspace;
no commit, push or remote CI execution was performed.

Initial red: planned types/API were absent, and tests failed compilation.
M1-009 separately failed against the first implementation after deleting the
journal tail; a transactional task event cursor corrected it. M1-004 synchronizes
the child at each exact commit boundary, kills it from the parent, and opens the
database without any other connection remaining to clean up the WAL. Its
`crash_child` helper is ignored in ordinary discovery but explicitly invoked
twice by the parent; no functional acceptance case is skipped.

Final commands (on this host, replace `cargo` with
`cargo +stable-x86_64-pc-windows-gnu`):

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --all-targets --locked
cargo test --workspace --locked
```

Results: all checks pass locally. Full feature suite: **25 passed, 0 failed**;
one ignored subprocess entry is explicitly exercised twice by M1-004. Default
suite: **9 passed, 0 failed**, with no store API or store integration suite enabled.
No performance numbers or power-loss/fsync fault guarantees are claimed.

## File inventory, review and remaining gates

Created: `crates/baton-store/{Cargo.toml,src/lib.rs,src/engine.rs,src/schema.sql,
src/engine/tests.rs,tests/metadata.rs}`, protocol metadata types/test files, and
this report. Updated workspace/dependency lockfile, protocol exports/errors,
README, CI, ROADMAP, PROTOCOL, TESTING, REFERENCES, M0 historical report and
DECISIONS. Only synthetic temp data was used; no repository snapshots, tokens,
raw sessions or user database were added to the workspace.

Security review: parameterized SQL, fixed Git read operations through argument
arrays, inherited GIT settings stripped, optional Git locks/fsmonitor disabled,
no shell, no external mutations, no automatic capture/daemon/network, bounded
accepted text and lease durations, task ACLs/capabilities, static errors and
separate reported intent. Explicit caller-supplied text is untrusted data and
never executes. Direct database access by the same OS account is outside this
capability boundary; SQLite history is not a cryptographic audit chain.

Portability review: safe Rust and std paths/processes, OS-selected null device;
Unix private creation modes/link rejection are implemented but not run here.
Windows Unicode/spaces, index-byte preservation, path aliases and real process
kill are tested. macOS/Linux/MSVC CI is configured, not run. Windows private ACL
and hardlink checks, native principal binding, secure capability persistence,
network filesystem detection, non-UTF-8/newline path support and adversarial
filesystem replacement races are unresolved. The engine remains experimental
and disabled by default. Do not point it at untrusted/shared state directories.

M1 metadata subcases are green on this host; the full R0 T-001/T-002/T-008
assertions involving accept/checkpoint/activation remain unimplemented until
M3/M4. All transfer/activation and VERIFIED continuation functionality is absent.
Native M0 validation also remains pending. The exact next implementation milestone
is **M2 — Content-addressed checkpoint engine** (FR-05/09/16; INV-03/05;
T-004/T-005). Recommend validating native CI before widening support, then proceed
with M2 as a local experimental slice without enabling writer continuation.
