# M4 plan and evidence

Status: experimental Windows simulated implementation; final gate assessment is
recorded in the successive-transfer section below. The complete-project goal
remains active. Earlier increment reports below are historical.

1. Close the local identity/storage boundary needed by a usable CLI/daemon:
   owner-checked private directories/files, safe credential recovery and scoped
   local IPC. Test widened ACLs/modes, links, wrong credentials and restart before
   exposing existing store APIs (FR-12/13, security trust boundaries 1–4).
2. Add persistent transfer identities and QUIESCING -> PREPARED -> RESTORING ->
   READY_TO_COMMIT -> COMMITTED -> ACTIVE transitions. Recompute safety facts in
   the coordinator; never accept client booleans as fencing/restore proof
   (FR-08/09, INV-02/07). Tracer: denied premature activation and unchanged source.
3. Connect snapshot/receiver validation, source freeze evidence, transactional
   epoch CAS, fresh-only activation permits, recovery and explicit quality
   diagnostics. Unknown effects and missing native/adapter/durability guarantees
   remain BLOCKED (FR-12/13, INV-03/04/06/08/09).
4. Add the single native `baton` CLI/daemon surface with human/JSON responses,
   explicit opt-in startup and bounded authenticated requests. No generic shell
   endpoint, MCP, frontend or cloud service (FR-08/12/13).
5. Run T-003/T-008/T-009, concurrency, stale epochs, corruption and actual process
   kills around durable transitions, plus default/all-feature workspace checks.

Prior implementation reports are historical. Existing experimental reservations
and serialized process/capability records are not activation proof. Complete M4
and product gates will not be declared passed from a state-machine test alone.

## Implemented local-entrypoint increment

`baton-platform` now supplies checked private state, OS principal identity,
exclusive coordinator locking and local IPC. `baton-cli` supplies one native
`baton` executable with an explicitly started foreground daemon. The executable
requires the `experimental` Cargo feature and installs no service or global
configuration. Receiver staging, ownership commit and activation remain to be implemented;
this is progress within M4, not its completion report.

On Windows, new state gets a protected owner-only inheritable DACL before data
is stored. Existing broad/uncertain permissions are refused. Parent directories
must be owned and non-writable by others; read-only parent access is permitted
while the new empty child is protected. Unix uses private modes and ownership.
State leaf links, reparse points, database/capability hardlinks and UNC state are
refused. Root-owned, protected Unix ancestor aliases may resolve the approved
root, including macOS /var. Existing user ACLs are never silently repaired.

Under an exclusive OS file lock, the daemon recovers a stable actor ID using its
actual SID/effective UID and rotates a random in-memory capability. No bearer
token is saved. Bootstrap and endpoint publication are audited with coordinator
clock high-watermark timestamps. This increment initially used schema 4; current
stores use schema 19 and refuse schema 1–18 without migration. Existing unbound
actors are not automatically adopted.

Windows transport uses owner-only, local-only named pipes. Clients request
READ_CONTROL, verify pipe owner, and use identification-only SQOS so a server
cannot impersonate them. Unix transport uses private sockets and checks peer
effective UID in both directions. Each boot gets a fresh locator. A persisted
locator or PID is not a liveness oracle; old socket files are not adopted or
removed during recovery. Compact Unix socket basenames use 64 locator bits,
with exclusive/no-overwrite bind and a 100-byte path bound. This is not an
authentication secret. No TCP listener exists.

The envelope is schema 0.1.0 with operation ID, expected epoch and strict command
decoding. Actor identity and supervisor facts cannot come from client JSON.
Allowed mutations include repository/task metadata, claims/heartbeats, intent,
forensic capture and fresh restoration. Actor creation, native run observations,
process execution, retention and transfer activation are unavailable to clients.
Local operation hashes use a separate trust domain from native embedding calls;
denied operations are cached and audited. Read pages and frames are bounded
(64 KiB requests, 16 MiB responses, 100 tasks/200 events per page), with at most
16 connections. Incoming frames and outgoing writes have deadlines; clients
bound the complete exchange. Windows pipe-busy waits occur only before any
request is sent. Lost responses never trigger automatic mutation replay.

Available CLI commands are init, daemon, create-task, claim, heartbeat, status,
events, checkpoint, restore and checkpoint-info, with human and JSON output.
Generated repository/task/checkpoint/restore identities default to the operation
ID so retry after a lost response retains the exact request. Snapshot replies
are revalidated against artifacts and receiver content. Edited receivers refuse
with INTEGRITY_FAILURE and retain the edit. All responses separately report
continuation_quality=BLOCKED and EXECUTION_NOT_ENABLED, even when the metadata
operation succeeds. Human errors include the corrective action.

## Current tests and verification

Windows GNU native tests cover M4-OS01–05, M4-IPC01/02, M4-ID01/02, M4-P01,
M4-CLI01–04 and the concurrent local-claim subcase of T-008. CLI tests launch
actual binaries, kill the daemon and restart it, preserving actor/request identity
and one persisted task/lease. They verify source preservation, snapshot and
fresh-worktree restoration, cached receiver-integrity checks, denial/audit of
forged supervisor observations, bounded frames and opt-in startup diagnostics.

An initial concurrent-client test reproduced pipe-instance contention; bounded
connection setup fixed it. The implementation does not retry transmitted writes.
A separate test expectation was corrected to the existing documented
INTEGRITY_FAILURE code for receiver edits; production integrity checks were kept.

The platform library, with all features and test targets, type-checks for
x86_64-unknown-linux-gnu and aarch64-apple-darwin. This is compile evidence only;
native Unix runtime tests and cross-account abuse tests remain unverified.

Final Windows GNU verification for this increment:

- Formatting check, all-feature Clippy with warnings denied, and all-target build
  passed with `--locked`.
- All-feature workspace tests: **79 passed, zero failed**; three ignored entry
  helpers are explicitly invoked by parent fault tests.
- Default-feature Clippy/build passed; workspace tests: **21 passed, zero failed**.
- Platform all-feature/all-target checks passed for Linux x86_64 and macOS arm64.
  These are type checks, not native runtime or full-workspace cross builds.
- The built `target/debug/baton.exe --help` ran successfully. No user database,
  snapshot, log or credential artifact was created in the repository.

Created `crates/baton-platform` (privacy, identity, IPC and tests),
`crates/baton-cli` (native executable, coordinator and actual-process tests),
`baton-protocol/src/local.rs` and its tests, `baton-store/src/engine/identity.rs`,
and this report. Updated workspace manifests/lockfile, store schema/dispatch/read
pages and tests, protocol audit/diagnostic contracts, README, architecture,
protocol, security, references, testing, roadmap and historical reports.
ADR-020 records the choices, scope and remaining gates.

## Durable transfer-intent increment

The strict schema-0.1.0 policy model now covers QUIESCING, PREPARED, RESTORING,
READY_TO_COMMIT, COMMITTED, ACTIVATING, ACTIVE and failure/recovery states.
Engine-supplied facts are plain Rust values, not client JSON. Four synthetic
model tests establish ordering, receiver authorization, no epoch allocation on
failed precommit checks, explicit degraded acknowledgement and no rollback after
commit. Newly discovered context loss after commit enters RECOVERY_REQUIRED.
These tests do not certify a process boundary or real transfer.

Production control implements only `transfer-request`, `transfer-preflight`,
`transfer-info` and `transfer-abort`, with the same human/JSON envelopes. Transfer
IDs default to operation IDs. A source actor with a matching unexpired metadata
reservation can request an already-authorized receiver actor; same-principal
intent is allowed. This does not establish a validated vendor receiver.

Schema 5 stores one pending transfer per task and append-only record versions.
The record hash is linked to the task event; all state, evidence, audit and retry
results commit together. Reads compare the mutable record with its journal and
task evidence. A damaged pending index fails closed. Pending intent denies new
source launch reservations, subsequent pending-permit validation and reservation
cancellation; it does not fence a running or concurrently spawning process.
Heartbeat, supervisor observations and forensic capture remain available.

Preflight verifies checkpoint bytes, manifest, actor and epoch, then records
BLOCKED reasons for missing fence, event seal, effect settlement, adapter and
isolation evidence. Windows artifact durability remains unproven. No stored
capability boolean is accepted as proof. Abort records FAILED_PRECOMMIT without
changing owner or epoch; it can close expired intent while leaving SUSPECT and
the original expiry intact. Historical retries never grant current authority.

Added `baton-protocol/src/transfer.rs`, its model tests and
`baton-store/src/engine/transfers.rs`. Updated protocol exports/action/event/local
contracts, SQLite schema/dispatch, launch validation, CLI dispatch/parser and
store/CLI tests. Updated README, roadmap, architecture, protocol, security,
testing and historical schema notes. ADR-021 records the experimental schema
compatibility break and authority boundary. No vendor interface changed.

M4-TR01–05 and M4-CLI05 exercise request/preflight/abort, authenticated binding,
stale epochs, concurrent intent, pending launch permissions, restart, actual
coordinator hard kill, commit-boundary kills, corruption and expired abort.
They are T-003/T-008/T-009 subcases, not complete transfer acceptance. An initial
snapshot test lacked a Git commit; seeding its synthetic repository fixed the
fixture. A test-first postcommit context-loss case exposed and fixed a policy
model downgrade bug. No production safety check was relaxed.

Cached preflight replies also revalidate the checkpoint contents. The corruption
retry test first reproduced a stale cached integrity result, then passed after
using the existing checkpoint verifier on those replies. Read-only transfer-info
reports historical persisted state and is not an integrity or authority grant.

Final verification for the durable transfer-intent increment (22 October 2026):

- Host: Windows NT 10.0.26300.0; Rust GNU 1.98.0; Git 2.51.0.windows.1.
  This workspace has no Git metadata, so no commit revision is available.
- `cargo +stable-x86_64-pc-windows-gnu test --workspace --all-features --locked`:
  **90 passed, zero failed**, three ignored subprocess entry helpers explicitly
  invoked by parent fault tests. Includes actual request commit-boundary kills,
  coordinator kill/retry, concurrent requests and corrupt preflight retry.
- `cargo +stable-x86_64-pc-windows-gnu test --workspace --locked`:
  **25 passed, zero failed**; experimental engines stay disabled by default.
- Formatting check, default and all-feature all-target Clippy with warnings
  denied, and all-feature/all-target build passed with `--locked`.
- Built `target/debug/baton.exe --help` ran and lists the four transfer commands.
  No real vendor agent, cloud endpoint or user snapshot was used. No new native
  Linux/macOS runtime evidence or performance benchmark is claimed.

Recommendation: continue M4's receiver staging and native proof work before M5.
M4's exit gate remains open; synthetic policy success does not enable execution.

## Artifact restore recovery increment

The protocol requires PREPARED before actual transfer receiver staging. Current
source-proof gaps still prevent that state. This increment makes the underlying
forensic artifact staging recoverable without bypassing that requirement.

The existing `restore` command now commits immutable intent and RESTORE_RESERVED
evidence before creating a worktree. The full request fingerprint binds actor,
task, epoch, lease, checkpoint/restore/operation identities and local/trusted
scope. The operation ID is reserved globally before final completion. After
restore validation, a schema-0.1.0 private immutable receipt binds that request
to its result. A retry after final-commit interruption can reuse only the matching
receipt after rechecking actual content. A target without a receipt is refused
even if its bytes match. Edited receivers, partial targets and corrupt receipts
are preserved. The original create-new-only Vault API stays unchanged.

Added `baton-store/src/engine/restores.rs` and the versioned RestoreReceipt type.
Updated store entrypoints, shared request fingerprinting, snapshot restore,
schema/identity checks, checkpoint layout/restore and tests. Schema 6 refuses
older stores without modification; there is no migration. No CLI flags, vendor
integration, dependency, service, frontend, MCP package or cloud feature changed.
ADR-022 records the filesystem/SQLite split and compatibility choice.

M4-RS01–03 use real temporary Git worktrees and actual process kills. The initial
receipt test failed to compile before implementation. A later regression first
reproduced operation-ID reuse between intent and completion; global reservation
now rejects and audits that conflict. All continuation outcomes remain BLOCKED.

Verification for this increment on the same Windows GNU host:

- `cargo +stable-x86_64-pc-windows-gnu test --workspace --all-features --locked`:
  **93 passed, zero failed**, with three subprocess entry helpers explicitly run
  by their parent fault tests. Seven observed restore crash boundaries were
  exercised across the receipt and SQLite tests.
- `cargo +stable-x86_64-pc-windows-gnu test --workspace --locked`:
  **25 passed, zero failed**; experimental engines remain disabled by default.
- Formatting check, default/all-feature all-target Clippy with warnings denied,
  and all-feature/all-target build passed with `--locked`.
- No native Unix runtime tests, power-loss tests, vendor validation or performance
  benchmarks were run for this increment. It uses portable safe Rust over the
  existing platform/filesystem checks and introduces no platform-specific branch.

Recommend continuing M4 proof sealing and PREPARED receiver staging before M5.
The complete-project goal and M4 exit gate remain open. Schema 1–5 state cannot
be opened by this experimental schema-6 build without a future migration.

## Native scope/drain evidence increment

The existing native process backend and LaunchSpec moved to the opt-in platform
`process` feature so the store and supervisor can share opaque live types without
a dependency cycle. Supervisor keeps ownership. Scope ID is a fresh random
identity bound to the original launch permission and actual launch fingerprint.
An opaque borrowed drain proof is available only after the owned backend reports
empty membership. No JSON flag, saved PID, RunRecord or capability boolean can
construct it. Windows job queries retain the existing M3 containment limits;
Unix process groups remain unable to mint this proof.

Native launch/drain commit in a scope-bound request domain, with immutable native
records, execution events, task hashes, audit and operation results in one SQLite
transaction. Exact metadata retries return historical results without another
process launch or permission. Session still checks authority before physical
spawn; the fresh native bind rechecks pending transfer, owner, epoch and lease.
Reads verify native binding, event hashes and terminal execution evidence. The
observed cursor is explicitly not a completeness seal. Serialized reports cannot
restore the live proof or control object.

Added `baton-platform/src/process.rs`, its moved native backend and
`baton-store/src/engine/native.rs`. Updated supervisor, protocol data/event types,
store native context and schema, feature/dependency wiring, lockfile and tests.
Schema 7 refuses schema 1–6 without migration. Dependencies moved at their pinned
versions; no new vendor integration, process-launch RPC, frontend or cloud
surface was added. ADR-023 records the boundary and module choice.

M4-NP01–04 use actual native jobs and synthetic temporary work. The initial native
read test failed to compile before implementation. A missing-execution-tail test
first reproduced a stale seal read, then passed with terminal journal validation.
An exact spawn-registration retry first returned BLOCKED; fresh-only validation
now runs after cache resolution while Session retains its pre-spawn check.
Unrelated scopes and synthetic status cannot create a seal. All continuation
quality remains BLOCKED; native drain alone does not permit PREPARED or activation.

Transfer preflight now attaches the validated native drain record, when present,
as `source_drain`, bound to the current source actor/epoch. Cached preflight
replies recheck its journals. The native stop -> checkpoint -> request/preflight
test verifies the report contains actual scope evidence while SourceFenceUnavailable
still blocks transfer. This field is diagnostic output, never client proof input.

Final verification for this increment on Windows GNU:

- All-feature workspace tests with `--locked`: **97 passed, zero failed**. Three
  ignored subprocess entry helpers are explicitly invoked by parent fault tests.
  Includes all existing daemon, artifact, native process and crash tests.
- Default workspace tests with `--locked`: **25 passed, zero failed**.
- Formatting check, default/all-feature all-target Clippy with warnings denied,
  and all-feature/all-target build passed.
- Platform all-target/all-feature type checks passed for Linux x86_64 and macOS
  arm64, explicitly enabling `blake3/pure` to avoid a foreign C toolchain. Commands:
  `cargo +stable-x86_64-pc-windows-gnu check -p baton-platform --all-targets --all-features --features blake3/pure --target x86_64-unknown-linux-gnu --locked`
  and the same command with `--target aarch64-apple-darwin`. These are type checks,
  not native OS runtime, full-workspace cross builds or default SIMD validation.
- No real vendor session, power-loss test, new benchmark, cloud upload or user
  repository snapshot was used. Source isolation, validated adapter/event coverage
  and external-effect settlement remain independent unresolved gates. Native
  Windows evidence is limited to the tested managed job scope.

Recommendation: continue M4's remaining safe-boundary and receiver staging work.
The complete-project goal remains active. Neither M4 nor the R0 exit gate is
passed by these native facts; schema 1–6 migration also remains unimplemented.

## Durable receiver/commit storage increment

Implemented receiver staging reservation, completion, metadata ownership CAS and
recovery. Begin-staging requires the designated receiver and PREPARED, and commits
the restore ID and reserved stage request fingerprint before files are created.
The stage command must use that operation ID/domain. Completion restores only in
the owned create-new location or reuses its bound receipt after validation.
Commit rechecks actual receiver content, source snapshot equivalence and source
lease expiry, then atomically advances epoch once and records receiver ownership
in COMMITTED. This is not ACTIVE and normal launch permissions remain forbidden.
Recovery/committed expiry retain the new owner and epoch. Task reads cross-check
the committed transfer so damaged ownership does not silently become authority.

Production PREPARED remains unreachable with the current missing proofs. No
request field, CLI option or runtime switch bypasses those checks. A private
task-scoped `cfg(test)` fixture supplies explicit external-boundary assumptions
only to exercise the durable state machine. CAS, Git restoration, receipts,
source/receiver checks, identity, SQL transactions and actual child kills are
real; fencing/isolation, adapter/event/effect and durability assumptions are not
production evidence. Receiver activation remains EXECUTION_NOT_ENABLED even in
that fixture. No complete M4/R0 or real handoff success is claimed.

Added `baton-store/src/engine/transfer_tests.rs`. Updated protocol actions/stage
records/task states, store transfer routing/reservations/checks, source comparison
in the checkpoint library, CLI commands and tests. Schema 8 preserves/refuses
schema 1–7 without migration. No dependency or vendor interface changed. ADR-024
records the scope and the test-only verifier boundary.

New CLI commands: transfer-begin-staging, transfer-stage, transfer-commit,
transfer-recover and transfer-activate. Staging requires the reserved global
`--operation-id`; beginning staging requires an explicit `--stage-operation-id`.
Source epoch is explicit. Commit's `--accept-degraded` cannot waive mandatory
guarantees. The actual daemon uses the production checker, refuses stage/commit
from BLOCKED, creates no receiver, and reports disabled activation.

M4-TX01–09 cover role/operation/destination binding, source drift, receiver edits,
concurrent commits, stale epochs, lost boundary assumptions, committed expiry,
recovery and task/grant corruption. Child processes are killed before/after
staging reservation, stage-result commit and ownership commit. Exact retries
preserve one receiver and one epoch. The first tests failed before interfaces
existed; an ownership-corruption test first reproduced a mutable-task/grant
mismatch, then passed after read-side correlation was added.

Verification for this storage increment on Windows GNU:

- `cargo +stable-x86_64-pc-windows-gnu test --workspace --all-features --locked`:
  **106 passed, zero failed**, three ignored entry helpers invoked by parent
  fault tests. Nine tests explicitly use the documented fixture boundary or
  verify the production checker remains closed. Six transfer commit boundaries
  use actual process kills; no power-loss or native receiver activation is tested.
- Actual CLI/daemon tests exercise the new command parsing and real production
  denials, then recover/abort without changing epoch or creating a receiver.
  Native process, artifact, identity, IPC and prior crash tests remain green.
- `cargo +stable-x86_64-pc-windows-gnu test --workspace --locked`: **25 passed,
  zero failed**. Formatting, default/all-feature all-target Clippy with warnings
  denied and the all-feature/all-target build passed with `--locked`.
- Native Unix runtime, real vendor adapters, native receiving-writer activation,
  power-loss durability and performance benchmarks remain unverified here.

Recommendation: continue M4 activation-permit and production-boundary work before
claiming a real handoff. The full project and M4 gate remain incomplete; all
production continuation responses remain BLOCKED. Schema 1–7 migration is absent.

## Receiver launch intent increment — 23 October 2026

Plan: bind trusted receiver launch input to the committed grant (FR-08,
INV-02/07, T-003/T-008), issue a fresh-only permit after durable intent and retain
epoch through failure/restart (FR-12, INV-06/07), then test actual native binding
without claiming a vendor handshake (FR-13, INV-08, T-009). Keep CLI activation
and missing production boundary proofs closed.

Implemented `Store::prepare_receiver_launch` with explicit native LaunchSpec,
run ID and capabilities. It authenticates the designated committed receiver,
verifies the actual receiver files and cwd, checks engine boundary requirements,
and resamples time after validation. A distinct idempotency domain binds the
whole request, launch fingerprint, run ID and capabilities. Schema 9 records the
pending RunRecord in its transfer and commits ACTIVATING, journal, audit and
retry result atomically before a fresh permit can be returned. Ordinary launch
requests cannot use this path, and local JSON cannot provide launch input.

Exact retries/restarts return historical run data without permits. Conflicting
parameters and trust domains refuse. Source run history is retained under the
new one-run-per-task/epoch constraint. Task/transfer reads verify activation
binding, fingerprint and capabilities against current run metadata. Late expiry
and supervisor loss retain the receiver/epoch in RECOVERY_REQUIRED; no replay or
old-source reactivation is provided.

The existing supervisor consumes the permit and binds the real native scope.
Pre-spawn validation rechecks actual receiver content and time. Post-spawn scope
binding checks current grant and pending run without requiring a pristine worktree
from the already running child. A process spawn alone leaves ACTIVATING/BLOCKED:
vendor ACTIVE requires a validated adapter/session handshake. Production proofs
remain missing, so production cannot issue a receiver permit. Positive tests use
only the documented private unit fixture assumptions. ADR-025 records this
boundary; no dependency or vendor interface changed.

Files changed: protocol metadata/transfer contracts; platform LaunchSpec cwd
accessor; store schema, transaction/expiry checks, execution, native binding,
transfer persistence/preflight and crash helpers; receiver tests and old-schema
refusal coverage; README and the protocol, architecture, security, testing,
roadmap, decisions and implementation reports. No new production module was
needed. No frontend, MCP, cloud backend, real vendor session or user snapshot
was created.

Test-first evidence: initial activation tests failed for missing API/contracts.
Subsequent behavioral tests exposed acceptance of corrupt run fingerprints,
expiry during launch validation, and supervisor loss left ACTIVATING. Each was
observed failing and fixed. M4-AC01–08 cover the corrected behavior plus actual
native spawn, wrong cwd, receiver edits, missing production proofs, retained
source history, duplicate/conflicting/restarted requests and concurrency.
Four actual hard kills cover both sides of activation-intent and native binding
commit. The fake receiver is the test binary with --help, not Codex/Claude.

Final verification (Windows build 26300, Rust 1.98.0 GNU, Git 2.51.0.windows.1;
no Git revision because this workspace has no .git directory):

- `cargo +stable-x86_64-pc-windows-gnu test --workspace --all-features --locked`:
  **114 passed, zero failed**. Three ignored subprocess entry helpers are
  explicitly invoked by parent fault tests. All eight activation tests, the four
  new hard-kill boundaries and prior IPC/artifact/process/transfer cases pass.
- `cargo +stable-x86_64-pc-windows-gnu test --workspace --locked`: **25 passed,
  zero failed**. Formatting, default/all-feature all-target Clippy with warnings
  denied, and all-feature/all-target build passed with --locked.
- Full-workspace Linux cross-check could not build bundled C dependencies:
  `x86_64-linux-gnu-gcc` is absent (zstd-sys/libsqlite3-sys). The sequential macOS
  workspace check was not reached. This is an unavailable foreign toolchain,
  not a successful portability check. Native Linux/macOS runtime is unverified.
- Platform-only all-target/all-feature checks with --features blake3/pure passed
  for x86_64-unknown-linux-gnu and aarch64-apple-darwin. These cover the native
  LaunchSpec accessor and platform types, not the whole engine or native runtime.
- Security review: launch input remains trusted embedding data; local JSON
  receives no command or proof field. Only a hash of launch parameters persists;
  command arguments/environment are not added to logs or transfer records.
  Existing private-state/CAS/cwd checks remain in use. No new process sandbox or
  same-user isolation guarantee is claimed. Actual CLI --help runs; no daemon or
  fake-agent process remains and no database/log/snapshot appeared in the repo.

Next at that increment: remain in **M4**, implement the simulated-receiver handshake and
finish its local simulated transfer/crash exit matrix. Production boundary and
vendor checks stay closed until established; a simulated M4 gate cannot certify
them. **M5 (real Codex source adapter)** follows M4; it is not started here.
Native Unix execution, Windows directory durability, encryption/privacy, schema
1–8 migrations and representative performance remain open. Continue M4; do not
claim a complete project, real transfer or passed M4 exit gate.

## Receiver readiness increment — 23 October 2026

Plan: normalize a versioned receiver acknowledgement, require the original live
native scope and committed run/lease, persist readiness without another launch,
then test stale/forged input and crashes (FR-08/12/13/15, INV-02/07/08/09,
T-003/T-008/T-009; M4-HS01–08). Keep missing production checks and vendor validation
closed. This increment completes the generic readiness transition; it does not
claim the full local simulated exit matrix or any vendor integration.

`ProcessTree::running_proof` samples a live root in its owned scope and returns a
nonserializable borrowed object. It promises neither future liveness nor full
containment. `Session::acknowledge_receiver` combines this with the normalized
schema-0.1.0 ReceiverSession, checks the binding and stops its owned tree on error.
Store requires the original immutable native binding, committed receiver and
epoch/lease, pending activation and a run with no earlier recorded tool sequence,
gap or effects. It rechecks engine boundary/artifact requirements and time.
Generic trusted JSON and the local allowlist cannot invoke readiness.

Schema 10 commits ACTIVE metadata, native/session readiness evidence, task event,
audit and retry result together. Session IDs are non-nil/unique and adapter fields
are bounded. Task reads reject ACTIVE without the audited readiness grant. A
normalized acknowledgement is not vendor validation: continuation quality stays
BLOCKED/RECEIVER_ADAPTER_UNVALIDATED. Release libraries cannot enable the private
boundary assumptions used by positive tests. Saved ACTIVE data never reconstructs
native control, future liveness or launch permission.

Tests drive a real fixture child, which reads a bound challenge, writes a known
local receiver change only after ownership commit, publishes its acknowledgement
without overwriting an existing response, and stays live until stopped or the
coordinator dies. Source bytes remain unchanged. These are real child/SQLite/Git
operations with explicitly assumed source boundary/adapter/durability properties,
not a real agent session, safety sandbox or context-continuity evaluation.

Test-first checks exposed and fixed: ACTIVE task metadata accepted without an
acknowledgement; historical commit retries rejecting legitimate receiver edits;
and a recovery event inserted before a drain seal breaking strict terminal
evidence ordering. The fix preserves that evidence check and writes recovery
after native persistence. Actual hard kills bracket readiness commit; restart
retains epoch/owner and denies serialized replay. Lost supervision and expiry
never return old source authority. Direct forensic restore still verifies bytes.

Files changed: protocol execution/metadata/transfer contracts; platform process
proofs; store schema, native context/readiness, transfer checks/journaling, cached
reply checks, expiry and crash helpers; supervisor acknowledgement and negative
integration test; store readiness/old-schema tests; README and specification,
roadmap, security, architecture, testing and implementation reports. No new
production module, dependency, CLI shell endpoint, frontend, plugin or cloud
component. ADR-026 records the quality distinction and authority limits.

Final verification on Windows build 26300 / Rust 1.98.0 GNU / Git
2.51.0.windows.1 (no Git revision: the workspace has no .git directory):

- `cargo +stable-x86_64-pc-windows-gnu test --workspace --all-features --locked`:
  **122 passed, zero failed**. Four ignored entries are subprocess helpers
  explicitly invoked by their parent tests, including the new ready child.
  All eight M4-HS cases and both new hard-kill boundaries passed, together with
  the earlier source/transfer/CAS/IPC/privacy/process tests.
- `cargo +stable-x86_64-pc-windows-gnu test --workspace --locked`: **25 passed,
  zero failed**. Formatting, default/all-feature all-target Clippy with warnings
  denied, and all-feature/all-target build passed with --locked.
- Platform-only all-target/all-feature checks passed for Linux x86_64 and macOS
  arm64 with --features blake3/pure. These type-check the new running proof;
  they are not native runtime, default SIMD or full-workspace cross builds.
  The Linux C cross-compiler remains absent, so full foreign workspace checks
  were not repeated. Native Unix runtime and three-OS release gates remain open.
- Security review: no serializable proof/launch bypass; acknowledgement binding
  and original scope are checked; only normalized metadata and native observation
  persist. Actor/task/epoch/lease and idempotency still apply. A running sample
  grants no claim about future liveness or sandboxing. Raw session content,
  secrets and model-private state are neither captured nor transferred here.
  CLI --help runs; no Baton/fake-agent process or repo database/log remains.

Next: continue **M4** with the complete local fake-agent transfer tracer and the
remaining first-receiver-tool/crash scenarios and subsequent transfer from an
active receiver, then assess its simulated exit
gate. **M5** remains the next milestone after that gate and supplies the real
Codex source adapter. Vendor compatibility, production source fencing/coverage,
native Unix execution, Windows directory durability, schema 1–9 migrations,
encryption/privacy and representative performance remain open.

## Successive transfer and first receiving tool — 23 October 2026

Plan: complete the M4 local simulated tracer with a receiver's first observed
tool and next handoff, retaining actual receiver edits and epochs. Test admission
before/after QUIESCING, crash uncertainty, current-workspace identity and stale
requests before enabling further adapters (FR-03/05/08/09/12/13/14,
INV-01–09; T-003/T-008/T-009 subcases M4-SEQ01–05, M4-TL01/02 and
M4-SEQ-CAS/SEQ-CAS02). Do not certify production source proofs from fixture data.

The current grant and pending transfer are separate. ACTIVE receivers can request
the next handoff and enter QUIESCING. The original grant/owner remains at its
epoch until a new conditional commit; a unique committed epoch prevents duplicate
grants. Admission stops immediately, while completion observations, heartbeat and
capture remain possible. A deliberately drained source does not prematurely
invalidate its grant. Expiry, uncertain loss or abort retains owner/epoch and
requires recovery; no prior writer silently resumes. Known gaps, effects and
live source scopes cannot be hidden by the test verifier's assumptions.

Current workspace resolution follows the audited grant's completed receiver stage
and original common Git directory. Receipt format 2 pins canonical administrative
Git identity. The dedicated read-only capture path accepts that exact receipt,
task/repository and next epoch. Ordinary capture still refuses vault overlap.
This addresses the originally observed UnsafePath failure without granting
arbitrary access inside CAS. Legacy receipts and unknown targets are preserved
and refused. Receiver edits become the next checkpoint; the original checkout
and earlier receiver worktree remain unchanged. Direct forensic restore retries
retain their byte-integrity check.

ToolReserved records durable intent separately from actual ToolStarted/Finished.
Only a fresh native-host transaction can issue an opaque ToolPermit. Scope,
canonical launch cwd, actor/task/run/epoch/lease, sequence, event gap, unresolved
effects and pending transfer are checked; consumption rechecks authority. Exact
retries and reopened stores return no permit. Initial support admits trusted
ReadOnly/LocalReversible classifications in task scope `.` only. Other scopes and
effect classes remain disabled. Host policy must constrain the concrete operation;
the permit is not an arbitrary shell tool, sandbox, or guarantee about the honesty
of an agent's classification. Late actual starts during QUIESCING remain recorded
facts and force recovery; unresolved reservations become UNCERTAIN on loss.

The typed fixture child waits for fresh admission before its known local file
write. Its start/completion is then observed. The A -> B -> A tracer captures that
edit, restores it into another new worktree, commits epoch 3 and rejects epoch 1
even when the same actor owns epoch 3. Four actual coordinator kills cover before
reservation intent, after intent, after write and after completion. An exclusive
child-held file handle plus job teardown supplies fixture loss evidence. Reopen
preserves absent/UNCERTAIN/COMPLETED facts and never replays a tool or adopts a PID.
These are real native process, Git, CAS and SQLite operations with explicitly
assumed missing source-boundary/adapter/durability properties, not vendor sessions.

Architecture choice/departure: retain the original module boundaries and one
native executable. Add a narrow receipt-bound capture exception and separate
admission from observations, rather than weakening ordinary path checks or
discarding late facts. ADR-027 records the rationale, compatibility and limits.
State schema 11 refuses schemas 1–10 without mutation; receipt format 2 refuses
older receipts. Neither has an automatic migration. Envelope/manifest schema
remains 0.1.0. No dependency, public shell endpoint, frontend, MCP, plugin or cloud
component was added.

Files changed across this increment: protocol metadata/execution/checkpoint types;
platform process cwd/proof accessors; checkpoint engine and snapshot tests; store
schema, identity, engine authorization, native/execution/snapshot/transfer paths
and subprocess/transfer tests; supervisor session admission and loss handling;
README, architecture/protocol/security/testing/roadmap/decision records, and
M1/M3/M4 implementation reports. No new production module or crate was created.

Verification environment: Windows 10.0.26300.0, Rust 1.98.0 GNU,
Git 2.51.0.windows.1. There is no Git revision: this workspace has no `.git`.
The final rerun follows one Clippy-only simplification combining two identical
EXECUTION_NOT_ENABLED branches; their authorization conditions are unchanged.

Final results after the lint simplification:

- `cargo +stable-x86_64-pc-windows-gnu test --workspace --all-features --locked`:
  **131 passed, zero failed**, exit 0. Four ignored subprocess entries are
  explicitly invoked by parent tests, not skipped acceptance cases. All nine
  new SEQ/TL/CAS cases passed, including four actual first-tool hard kills.
  Local evidence: `%TEMP%/baton-m4-schema11-verified-tests.log`.
- `cargo +stable-x86_64-pc-windows-gnu test --workspace --locked`: **25 passed,
  zero failed**, exit 0. Evidence: `%TEMP%/baton-m4-schema11-default-tests.log`.
- Formatting check, default and all-feature/all-target Clippy with warnings
  denied, and all-feature/all-target workspace build passed using --locked.
  The initial Clippy duplicate-branch failure was fixed, not suppressed.
- Platform-only all-target/all-feature checks passed for Linux x86_64 and macOS
  arm64 with `--features blake3/pure`. These are type checks, not native OS tests,
  full foreign workspace builds, SIMD validation or process-containment evidence.
  The previously observed missing Linux C cross-compiler was not reinstalled or
  retested. Native three-OS release gates remain open.
- Security/portability review retained ACL/privacy, path checks, immutable CAS,
  native proof identity, epoch/idempotency and forensic retry validation. No
  credential, raw session, user repository snapshot or launch replay recipe was
  added. CLI --help runs. No repository database/log or live fixture process
  remains after checks. The separate visual preview used only synthetic temp data.

Gate assessment: the **bounded Windows local simulated M4 exit gate passes**:
T-003/T-008/T-009 simulated subcases, real fixture handoff including first tool
and successive transfer, and deliberate coordinator crashes are exercised. This
is not the full R0 gate or a production transfer guarantee. Experimental features
stay opt-in, production PREPARED/activation remains closed, and native OS release
validation remains pending. A simulated gate cannot establish real vendor support.

Exact next milestone: **M5 — Real Codex source adapter**. Proceed with public
App Server compatibility discovery and a test-first supervised source slice;
real source sessions and event coverage must supply their own evidence. M6 then
supplies the real Claude receiver and context packet. Do not require those later
vendor proofs as prerequisites for finishing M4's explicitly simulated gate.
Open constraints: native Unix containment/runtime, Windows directory durability,
full workspace matrix and performance, privacy/encryption of sensitive evidence,
schemas 1–10 migrations, real adapter capabilities/source isolation and the real
E2E-A/R0 acceptance suite. The complete-project goal is not achieved.
