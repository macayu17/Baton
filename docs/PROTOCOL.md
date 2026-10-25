# Baton Continuation and Transfer Protocol (BCTP)

**Version:** 0.1 proposed (not a finalized external standard).

## 1. Guarantee boundary

Baton promises verifiable restoration of **captured observable execution state** with evidence-backed context and guarded ownership. It does **not** transfer model-private computation, guarantee identical next tokens, or provide unconditional exactly-once execution of external side effects.

Continuity tiers:
- **C1 Workspace:** Git base, included byte content, index state, file metadata within documented OS limitations.
- **C2 Execution:** ordered events, tool results, check outcomes, latest acknowledged safe point, unresolved effects.
- **C3 Decisions:** explicit user constraints, cited agent-reported decisions, documented rejected approaches; incomplete implicit intent is never presented as fact.
- **C4 Hidden model:** not supported.

## 2. Non-negotiable invariants

- **INV-01:** At most one *authoritative coordinator-issued* writer lease for each task epoch; every transfer monotonically increases epoch.
- **INV-02:** No receiving writer may execute before receiver restoration validates AND ownership commits.
- **INV-03:** No acknowledged checkpoint points to unavailable/unverified required artifacts.
- **INV-04:** No unknown non-idempotent external operation is replayed automatically.
- **INV-05:** A restored worktree never overwrites the source or unrelated user files.
- **INV-06:** A lease timeout without process/resource fencing never automatically establishes old-writer inactivity.
- **INV-07:** All transfer requests and retries are idempotent with persistent operation ID.
- **INV-08:** Safety downgrade (VERIFIED -> DEGRADED/BLOCKED) is explicit and auditable.
- **INV-09:** Gaps in observable event sequences and uncertain side effects are surfaced, not fabricated.

**Clarification:** INV-01 is a guarantee about the coordinator and instrumented mutation paths; arbitrary shells, untrusted IDEs and external cloud APIs require stronger isolation or cannot be fully fenced. Never market the system as preventing all external writes solely through a DB lease.

## 3. Persisted lifecycle

### Implemented M1 pre-execution metadata (experimental)

Before the execution lifecycle below, the M1 store supports `UNCLAIMED`,
`RESERVED`, and `SUSPECT`. A reservation permits only engine metadata operations;
it is not an ACTIVE writer or a VERIFIED recovery result. Initial claims advance
epoch from 0 to 1. Only an unexpired, never-activated reservation can be cancelled
by its matching owner/epoch/lease; a subsequent metadata claim advances epoch
again. Expiry preserves owner/epoch and marks SUSPECT on the next relevant
authorized mutation. It never permits cancellation, renewal, or new ownership.
Reads expose persisted state and expiry; they do not run a background scanner.

Request/record JSON uses schema 0.1.0. Mutations bind actor capability, operation
ID, action/task, and expected epoch; timestamps come from the coordinator clock,
sampled after obtaining the writer lock. A persisted clock high-watermark prevents
backward clock movement from undoing observed expiry. Exact retries return the
historical result without changing current state; a returned old lease response
is not proof of current authority. Transfer and activation messages below are
not implemented in M1. See ADR-017 and `M1-IMPLEMENTATION.md`.

### Execution lifecycle (M3/M4 and later)

M3 implements pre-activation run states LAUNCH_PENDING, RUNNING, STOPPING,
STOPPED and RECOVERY_REQUIRED, separately from task ownership. It does not yet
implement the transfer lifecycle below. Opening a run commits intent before a
nonserializable launch permit is returned to trusted embedding code; retries
never issue a second permit. An interrupted launch remains ambiguous and cannot
be replayed. PID values are diagnostic identifiers, never recovery authority.

Run observations and source cursors commit with task events and audit. Skipped
source intervals are sticky; interruption acknowledgement never marks a tree
stopped. An observed empty Windows job records process drain, not resolved remote
effects or snapshot fidelity. Unix process-group exit remains uncertain. Started
effects without completion become UNCERTAIN on stop/loss. Unknown and external
classes require resource reconciliation even after an observed tool success;
hash-only reconciliation is disabled. No automatic replay API exists.

M3 stores use database schema 3 and refuse schema 1/2 without migration. Run APIs
are trusted in-process primitives, not an agent/IPC surface. Session restricts
adapter notifications to adapter observations; future daemon authorization must
keep supervisor observations inaccessible to clients. Existing metadata cancel
cannot release a reservation that has a run. M4 must supply verified source
isolation, complete events and durable transfer authority before activation.

Task ownership/control states:

1. **ACTIVE:** current owner holds epoch-bound capability, supervisor established.
2. **QUIESCING:** source asked to stop new operations; in-flight operations assessed.
3. **PREPARING:** checkpoint material is staged; hash policy applied.
4. **PREPARED:** immutable checkpoint verified and transactionally published, source remains frozen/restricted.
5. **RESTORING:** receiver stages into new worktree with *no write authority*.
6. **READY_TO_COMMIT:** receiver validation succeeds; source fenced, ownership preconditions satisfied.
7. **COMMITTED:** atomic owner compare-and-swap increments epoch, invalidates old coordinator authority.
8. **ACTIVE:** receiver activated under new epoch after verifying the committed grant.
9. **RECOVERY_REQUIRED:** unsafe, ambiguous or failed operation demands reconciliation; no automatic writer activation.

Treat `COMMITTED` and receiver `ACTIVE` as separate persisted states: receiver can crash after commit and restart without handing ownership back to old source. Denote transfer terminal outcomes `SUCCEEDED`, `FAILED_PRECOMMIT`, `BLOCKED`, `RECOVERY_REQUIRED`; don't reuse incomplete IDs.

## 4. Messages and expected semantics

M4's implemented local envelope uses schema 0.1.0, request ID, expected epoch and
a strictly decoded command. Actor identity is supplied by the daemon's checked
OS scope, never by client JSON. The store separates local and trusted-embedding
idempotency fingerprints, denying and auditing supervisor/actor operations from
local control. CLI-generated resource IDs default to the operation ID; exact
retries across daemon restart therefore preserve request identity. A successful
metadata result still carries continuation_quality=BLOCKED. Current local
commands can request metadata commit behind engine-owned proof checks; process
activation remains unavailable over local control.

State schema 15 binds the stable local actor to the OS principal and journals
transfer intent/preflight/abort atomically with task events and retry results.
Schema 1–14 databases are preserved and refused; no migration is implemented. A new daemon
under an exclusive state lock rotates only the in-memory capability; no bearer
credential is saved. The persistent session ID locates an endpoint and does not
prove a daemon is alive. Connection failure reports DAEMON_UNAVAILABLE; loss
after transmission reports RESPONSE_LOST, preserving uncertainty about commit.

The experimental local `RequestTransfer` requires an unexpired RESERVED or ACTIVE
owner, matching lease/epoch and an authorized receiver actor. It records
QUIESCING intent, not an observed stopped boundary. An ACTIVE task or a task with
a started run enters task state QUIESCING; a metadata-only reservation stays
RESERVED. One pending transfer per task
denies new source launch reservations, subsequent launch-permit validation and
reservation cancellation, while source observations, heartbeat and forensic
capture remain possible. `PreflightTransfer` binds a verified checkpoint manifest
and reports missing production proofs as BLOCKED. It cannot commit an epoch.
`AbortTransfer` records FAILED_PRECOMMIT even after reservation expiry, with the
source actor and epoch still required; it neither revives the lease nor starts a
process. `Transfer` reads expose the persisted report. Exact retries return the
historical operation result, which is never a current authority grant.

Schema 11 distinguishes a pending handoff from the current committed writer
grant. An ACTIVE grant remains authoritative at its epoch while a subsequent
transfer occupies the pending slot. A unique committed epoch prevents a second
grant for that epoch. QUIESCING denies new tool admissions and permits observed
completion, heartbeat and capture. Abort never silently resumes a stopped writer:
it leaves SUSPECT or RECOVERY_REQUIRED, preserving any committed receiver/epoch.
Expiry or lost supervision likewise cannot return ownership to an earlier source.

The current source workspace is derived from the audited current-epoch grant's
receiver stage and immutable completion receipt, or the enrolled original root
before any commit. Receipt format 2 pins the canonical Git administrative directory;
the enrolled common Git directory must still match. A dedicated read-only capture
path accepts only that completed receiver, matching task/repository and the exact
next epoch. Ordinary capture continues to refuse vault-overlapping roots. Legacy
receipt formats and uncertain targets are preserved and refused. This permits
legitimate receiver edits to become the next source without overwriting history.

Tool admission is separate from observation. `ToolReserved` persists intent and
can issue one nonserializable ToolPermit only from a fresh transaction with the
original sampled native scope, authorized cwd, current actor/task/run/epoch/lease,
next normalized sequence and no gap or unresolved effect. Retrying returns data
without a permit; consumption rechecks current authority. Only trusted-host
ReadOnly/LocalReversible classifications and task scope `.` are admitted; other
classes/scopes stay disabled. No shell command or persisted replay recipe is
accepted. The host must enforce the concrete operation; this is not a sandbox.
Actual ToolStarted/Finished remain observations. A late start during QUIESCING is
retained and forces source/transfer recovery rather than being discarded as a
denied permission request. Unfinished reservations become UNCERTAIN on scope loss.
NativeScopeRecord now records canonical launch_worktree. See ADR-027.

The independently tested transfer policy model includes receiver validation,
commit and activation ordering with synthetic facts. It is not an execution
endpoint. Receiver staging and ownership CAS use the engine checks below; complete
production boundary proofs remain unavailable. Native receiver launch permits
are available only through trusted embedding, not local control. No client may
submit fencing, integrity, event-completeness or effect-settlement booleans.

All mutations include: protocol version; task ID; transfer ID or operation ID; actor ID; expected epoch; authentication/capability; sequence/idempotency key; timestamp for diagnostics. Timestamps do not grant authority; epochs and transactions do.

| Operation | Precondition | Durable result | Idempotent retry behavior |
|---|---|---|---|
| `ClaimTask` | Task unowned/eligible, actor authorized | Lease and epoch | Same key returns same lease; conflicting claim loses |
| `Heartbeat` | Matching current lease, epoch and owner | Renewed lease expiry | Repeating old heartbeat never resurrects a stale epoch |
| `RequestTransfer` | Matching active owner or authorized requester | Transfer record and QUIESCING | Return existing transfer ID/state |
| `ReportQuiescence` | Supervisor evidence and complete event cursor | Frozen source boundary | Reject fabricated/unobserved quiescence |
| `PublishCheckpoint` | Staged immutable artifacts validate | Manifest root + PREPARED state | Reuse published content hash and transfer ID |
| `StageReceiver` | PREPARED, valid receiver identity | Isolated restored worktree + verification report | Never overwrite; reuse same staged instance safely |
| `CommitTransfer` | Prepared, validated, source fenced, no unsafe effects | Atomic owner/epoch change | Return committed epoch on retry; reject stale competing transfer |
| `ActivateReceiver` | Matching committed receiver epoch | Supervised receiver ACTIVE | Duplicate activation must not spawn duplicate unmanaged writers |
| `AbortTransfer` | Not committed, safe rollback context | Previous owner stays stopped/eligible for explicit resume | Never undo committed authority implicitly |
| `RecoverTransfer` | Privileged recovery request | New verified, degraded or blocked disposition | Recovery is idempotent and audited |

## 5. Happy-path handoff (Codex -> Claude)

The experimental M4 store implements durable receiver staging, metadata ownership
commit and recovery behind engine-owned safety checks. Production PREPARED is
still unavailable because source fencing/isolation, validated adapter/event
coverage and effect settlement are missing. A private `cfg(test)` fixture supplies
explicit closed-boundary assumptions to test the storage transitions; no release
feature, environment switch, request field or CLI option enables that fixture.
These tests are not proof that the missing production guarantees exist.

`BeginReceiverStaging` requires the designated receiver and PREPARED. It commits
RESTORING, a create-new restore ID and a reserved staging operation ID/fingerprint
before filesystem work. `StageReceiver` must use that exact ID and request domain;
it restores via the existing completion-receipt path and validates actual bytes.
No receiver process is launched. Staging reserves both operation and destination
identities against unrelated actions and forensic restores.

`CommitTransfer` rechecks the engine's requirements, receiver content, source
snapshot equivalence and unexpired source reservation. One SQLite conditional
update advances epoch and installs the receiver in COMMITTED, with a fresh pending
lease. Task ownership must agree with its audited committed transfer. Duplicate
commits return the historical result; competing old-epoch commits fail. COMMITTED
is not ACTIVE: ordinary launch permits remain forbidden, and local ActivateReceiver
returns EXECUTION_NOT_ENABLED. Receiver expiry or explicit recovery preserves the
committed owner/epoch in RECOVERY_REQUIRED. Source abort cannot undo that grant.

The trusted `prepare_receiver_launch` embedding API accepts an explicit native
LaunchSpec, run ID and capabilities, never serialized command replay. It checks
COMMITTED receiver identity/epoch, actual restored workspace, mandatory boundary
checks, capability prerequisites and lease expiry after file verification. The
request fingerprint includes the launch descriptor, run ID and capabilities in
a separate domain. A transaction persists ACTIVATING, the pending run, journal,
audit and operation result before issuing a fresh in-memory launch permit. Exact
retries return historical data without a permit; a restart cannot reconstruct it.
Schema 9 permits one run per task/epoch so source history is retained.

The existing supervisor consumes that permit and binds the real native scope.
Pre-spawn validation rechecks receiver contents, engine proofs and lease expiry;
post-spawn binding checks current authority without requiring a still-pristine
workspace from the running child. Native spawn is not a vendor handshake: task
and transfer remain ACTIVATING/BLOCKED until readiness is recorded through the
trusted host path below.
Lost permissions, process errors or restart never authorize replay or old-source
reactivation. Recovery preserves the committed owner and epoch. All positive
activation-storage tests still use the private closed-boundary fixture; the
production checker cannot issue a receiver permit.

Schema 10 adds normalized `ReceiverSession` acknowledgement and a historical
`ReceiverReadyRecord` binding it to the native scope. `AcknowledgeReceiver` is
denied over local control; generic trusted JSON execution also cannot invoke it.
The trusted Session method supplies an opaque sampled-running-root proof from
its owned process. The store requires its original scope/PID/fingerprint,
current receiver actor/epoch/lease, pending activation binding and unexpired
lease. The run must be RUNNING without earlier tool events/effects or gaps.
Session IDs are non-nil and unique; adapter metadata is bounded.

After rechecking engine boundary/artifact requirements, acknowledgement commits
ACTIVE metadata, readiness evidence, task event, audit and retry result together.
The scope record is a native observation; the session describes what the adapter
acknowledged. Neither proves vendor compatibility. Continuation quality remains
BLOCKED with RECEIVER_ADAPTER_UNVALIDATED. Production boundary checks still deny
the launch path, and positive readiness tests retain private fixture assumptions.
No CLI command or runtime switch enables the test adapter.

The running-root proof is a sample, not future liveness, full containment or a
restorable capability. A saved ACTIVE result cannot reconstruct native control
or launch permission. Readiness errors stop the owned Session and record loss;
expiry/loss/stop preserve the committed owner/epoch in RECOVERY_REQUIRED. Native
drain seals persist before the transfer recovery event so terminal evidence
retains its strict journal ordering. Exact cached staging/commit replies remain
historical after activation intent: checkpoint hashes still validate, while
legitimate receiver edits are not compared to the old pristine snapshot. Direct
forensic-restore retries continue to verify current receiver bytes.

1. Codex runs under Baton-managed supervisor in an isolated task worktree and valid lease.
2. Baton observes tools, collects explicit requirements and event evidence, tracks any mutable side effects.
3. Transfer requested to Claude. Source adapter stops scheduling new actions; the supervisor requests supported graceful turn interruption and waits for a safe drain.
4. If child processes, shell actions or remote side effects are unresolved, enter RECOVERY_REQUIRED and do not assume safe completion.
5. Snapshot service validates Git state and file scope, hashes and stages blobs, fsyncs as appropriate, writes immutable manifest and records event cursor.
6. SQLite publishes checkpoint root atomically after durable artifact verification. No ownership change yet.
7. Receiver adapter initializes isolated worktree with limited read-only preparation permission, restores content and staged state, and compares manifest hashes/environment prerequisites.
8. Coordinator confirms source is fenced and effects reconciled. In one transactional conditional update, change owner and advance epoch.
9. Receiver adapter verifies ownership grant, injects continuation packet/evidence tools and starts Claude under managed scope.
10. Audit transfer completion with source/receiver agent versions, epochs, checkpoint ID and guarantee status.

Do not depend on agent-generated text to affirm process quiescence. A successful Codex `turn/interrupt` response proves request acceptance/turn interruption per vendor semantics; supervisor must additionally assess descendant commands and external effects.

## 6. Definition of a safe boundary

The experimental native path additionally binds a fresh random scope ID and the
actual launch fingerprint/PID to the pending run. This can be recorded only with
the live platform process-tree object. Native drain uses a nonserializable proof
borrowed from that same owned scope after its membership query reports empty.
Ordinary observation calls, saved RunRecord flags, PIDs and capability booleans
cannot create these immutable native records. Native operations use a separate
scope-bound idempotency domain. Scope binding and drain seals commit atomically
with execution events, task evidence hashes, audit and operation results.

Historical NativeDrainRecord data verifies its binding, task-event hashes and
terminal execution event on read. Its cursor is only the highest observed source
cursor, not proof of complete coverage. It says nothing about escaped services,
unmanaged writes, external effects or adapter validity. Windows can attest an
empty owned job; the Unix process-group backend cannot mint this proof. These
facts alone do not advance PREPARED or grant a receiver writer.
Transfer preflight includes `source_drain` only after validating that run's native
record against the current source actor/epoch and its journals. Cached preflight
replies recheck that evidence. This diagnostic field is output data; requests
cannot supply it as proof or use it to waive missing source-isolation guarantees.

A safe boundary is an event cursor `N` where all required file writes before `N` are captured consistently, source is no longer allowed to initiate new task writes, no uncontrolled descendants remain with applicable write ability (or their effect is isolated), and every started potentially mutating action is either observed complete, reconciled, or explicitly marked unresolved.

The system can checkpoint an **unresolved** boundary for forensics, but must not label it VERIFIED or automatically activate another writer. Consider read-only exploration a different permission level than write continuation.

For each observed action maintain an effect class: read-only; local idempotent/reversible; local uncertain; external verifiable; external non-idempotent; unknown. Default unknown and inherited shell subprocesses to conservative classification. Replaying completed actions is not continuation; restoration of resulting files is preferred. External effect replays require explicit resource-specific idempotency/reconciliation support.

## 7. Snapshot publication and recovery

### Implemented M2 artifact boundary (experimental)

M2 publishes a schema-0.1.0 forensic manifest root into database schema 2 only
after validating all captured blobs. It does not enter PREPARED, freeze a source,
commit ownership, or activate a receiver. Exact request retries revalidate
artifacts (and receiver files for restore replies) before returning old success
records; a missing/corrupt artifact is an explicit error, not a cached success.
All capture/restoration records report BLOCKED with SUPERVISION_UNAVAILABLE;
CAPTURE_GAPS and Windows DURABILITY_UNPROVEN are additional explicit limitations.
This is not a durable VERIFIED checkpoint acknowledgement on Windows.

Index and worktree state are independent. Nullable file-state fields must be
present; missing fields cannot silently mean deletion. Manifests bind task,
repository, actor, operation, epoch, parent checkpoint and observed event cursor.
Restoration uses --no-checkout and imports only included validated versions into
a fresh engine-owned worktree. Secret/ignored gaps are never materialized through
an intermediate base checkout. Failed/partial receivers are preserved; retries
cannot overwrite them. Artifact restoration grants no writer permission.

See `M2-IMPLEMENTATION.md` for flush/link ordering, fixed capture limits, refusal
cases and tests. Production durability and execution invariants remain gates.

State is intentionally split between immutable object storage and SQLite. Prepare blobs and manifest under a staging identifier; verify content and durability; transactionally make the manifest live. Stage orphan blobs are safe garbage and subject to garbage collection after grace period. Missing required blobs after a published root indicate corruption and block restoration.

The source remains logically frozen after PREPARED. If receiver staging fails, the transfer stays pre-commit and may retry with same key. If coordinator crashes after commit but before receiver activation, recovery must preserve *new* epoch; never silently reactivate source.

## 8. Lease fencing and split brain

M4's artifact restore commits immutable SQLite intent and RESTORE_RESERVED
evidence before any new receiver filesystem work. Intent reserves the full
operation ID, authenticated actor, task, epoch, lease, checkpoint, restore ID and
local/trusted request domain. Completion still rechecks ownership and expiry.
After complete restoration and validation, the engine publishes a schema-0.1.0
completion receipt in private storage, bound to the original request fingerprint.
A retry after a coordinator crash may reuse that receipt only after revalidating
the manifest, Git identity/index and every included receiver file. This does not
overwrite or reconstruct a worktree. Missing/corrupt receipts or edited targets
refuse; matching bytes without a receipt do not establish engine ownership.
Lost receipts/partial targets are preserved for inspection. Intent alone is not
completion and never permits writer activation. All outcomes remain BLOCKED.

**Local R0:** SQLite serializes authoritative lease transitions. Use monotonically increasing epoch; heartbeat with bounded timeout. Lease expiry can transition metadata to `SUSPECT`, but **does not permit automatically installing a new writer** without verifiable supervisor stop/namespace isolation. All Baton-coordinated mutations require matching epoch and current ownership. For stronger protection isolate task worktrees and managed command execution; a hook-only agent can be labeled cooperative, not strongly fenced.

**Distributed R2:** Server-side PostgreSQL is authoritative for shared task ownership. Client clocks are non-authoritative. A network partition or host crash means the server may mark lease `SUSPECT`; new owner needs external isolation/fencing proof or explicit human recovery with limited guarantees. Old host's local files may still be writable while offline; never assert global single-writer unless shared-resource gates enforce epochs.

## 9. Outcomes and degradation

- **VERIFIED:** all mandatory captured artifacts and restored environment scope verified; safe mutation boundary; valid new epoch; relevant effects resolved.
- **DEGRADED:** file state is safely restorable but specified optional context or reproducibility requirements are incomplete; no known unsafe effects; user must accept limitations before write continuation.
- **BLOCKED:** active/unfenced writer, mismatched hashes, missing required objects, unsafe effect, incompatible adapter, ambiguous ownership, or untrusted checkpoint content. Read-only inspection may still be possible.

Include structured `reason_code`, human description, evidence references, missing guarantees and suggested recovery steps. A user may not override cryptographic corruption or an unresolvable active writer merely by accepting a warning.

## 10. Crash/retry matrix

| Crash/partition point | Required recovery behavior |
|---|---|
| Before quiescence | Old ownership remains but status SUSPECT on crash; assess processes/effects |
| During CAS blob writes | Remove/retain orphan staging; never publish incomplete manifest |
| After checkpoint publication, before receiver | Retry receiver staging with same transfer ID; source cannot continue unnoticed |
| During receiver worktree restoration | Clean only Baton-owned staging worktree or resume verified stage; don't touch user checkout |
| After receiver validates, before epoch commit | Retry compare-and-swap; no receiver writer yet |
| After epoch commit, before activation ack | New owner remains authoritative; retry activation idempotently |
| Old source sends late heartbeat | Reject by epoch; log stale-owner attempt |
| Lost heartbeat due to network partition | Suspend remote write authority; no blind failover |
| Unknown deployment or migration result | Block and reconcile with external service, then resume explicitly |
| Storage corruption discovered later | Mark affected checkpoints unusable and revert to previous validated root if safe |

## 11. Compatibility and future extensions

M5 schema 12 adds AdapterReported and AdapterGap observations to the execution
journal. AdapterReported binds bounded adapter/version, thread, turn and optional
tool identity to the existing authenticated RunBinding. Task evidence hashes bind
the full execution event; reads and cached acknowledgements recheck those hashes.
Turn/item ordering is reconstructed from this journal rather than mutable caller
state. Known missing predecessors set sticky EventGap; conflicting identities
refuse. An orphan completion retains an uncertain effect, not an invented start
event. All current vendor command/file effects are Unknown: reported success does
not settle them. Supervisor loss also marks an unconfirmed observation tail.
Turn completion does not prove native drain or change writer ownership.

The current capture API accepts turn/started, turn/completed and command or
file-change item/started/item/completed, plus the passive metadata and cancellation
subset below. It retains reported exit code/outcome without raw command, stdout,
reasoning or message contents. Unsupported items, notifications and server
requests terminate capture with gap/loss. This is partial lifecycle coverage, not a production source
adapter. A deterministic, run-scoped operation key deduplicates normalized facts;
reusing that key with different facts is an audited conflict. Reopen may return
historical metadata; it cannot adopt a process or issue execution permission.
That increment used schema 12; current schema 15 preserves/refuses schemas 1–14.
Envelope schema stays 0.1.0. See ADR-029–033.

Schema 13 retains start/completion metadata for userMessage, agentMessage,
reasoning, plan and contextCompaction items. It stores no content or hidden model
state and creates no tool effects from passive items. Missing starts remain gaps.
Only command/file-change approval requests are recognized. Baton commits
ApprovalCancellationPlanned before writing a cancel reply and commits
ApprovalCancellationWritten after write/flush. Both have HostReported origin;
neither proves peer receipt or absence of an effect. A matching
serverRequest/resolved becomes AdapterObserved ApprovalResolutionReported,
correlated through the durable plan because the vendor notification has no turn ID.
Effects remain Unknown/UNCERTAIN. No positive approval or policy amendment exists.

RPC IDs retain string versus signed integer identity and bind the authenticated
run, adapter, thread, turn and item. Changed request facts conflict; reuse of a
resolved ID is unsupported and stops capture without another reply. If offered
decisions omit cancel, or the method/scope is unsupported, close the owned scope
with gap/loss rather than choose an execution grant. Failed storage preserves only
committed phases; cleanup cannot guarantee a final audit when storage is unavailable.
The positive deadline of at most 30 seconds bounds I/O waiting, not synchronous
transactions or cleanup. Successful hosts still stop explicitly. See ADR-030.

Schema 14 adds TurnInterruptPlanned/Written/Acknowledged, scoped to the live run,
adapter, thread and observed active turn. A deterministic request ID allows one
live request per turn. Planned is HostReported intent and moves the run to STOPPING;
Written is HostReported pipe write/flush, not delivery proof. A matching empty
success response is AdapterObserved acknowledgement, never native drain or effect
settlement. The reply may arrive before or after turn/completed. Interleaved
supported lifecycle messages retain their normal provenance. Unsupported or
uncorrelated replies, mixed result/error, rejection, timeout and cancellation
fail closed with gap/loss where storage permits. Error payloads are discarded.

An exact live retry with committed written evidence returns current metadata
without sending again. Intent without written evidence is ambiguous and cannot
be replayed. No saved operation can reconstruct a transport or process permit.
Storage failure may prevent final loss journaling; only committed phases are facts.
The supervisor must still independently stop/query its owned scope. Neither a
terminal turn nor an acknowledgement resolves Unknown tool effects. This remains
an experimental trusted embedding API, not real source-work certification. See
ADR-031 and M5-I01–06.

Schema 15 adds CallbackRefusal with request ID, opaque correlation ID, callback
kind, raw frame BLAKE3 fingerprint and Planned/Written/Resolved phases. Planned
and Written are HostReported; Resolved is AdapterObserved. Approval and callback
server requests share phase keys, preventing the same RPC ID from rebinding across
methods. Changed payload/scope conflicts, resolved-ID reuse refuses, and Written
cannot follow resolution. Fingerprints detect changes to ignored payloads without
persisting their text; they are private metadata, not encryption or vendor proof.

For exact item/tool/requestUserInput, persist Planned then stop with
EXECUTION_NOT_ENABLED and gap/loss. Do not invent an answer, cancel choice or
Written phase. Only this method spelling from the generated schema is supported.
For item/tool/call, persist Planned before writing a fixed success:false result
with BATON_EXECUTION_NOT_ENABLED text, then Written after flush. No tool or arbitrary
argument is executed. Optional serverRequest/resolved correlates through that plan;
it is not proof of delivery or effects. Dynamic item start/completion is bounded
ClientToolCall metadata, not an observed host tool effect or execution grant.
Unsupported/malformed requests still stop capture. Native stopped scope, effect
reconciliation and source activation remain separate. See ADR-033.

The initial experimental M5 handshake implements the version-pinned public
initialize/initialized exchange over native owned JSONL pipes. Frames are bounded
to 64 KiB and the handshake has a positive deadline of at most 30 seconds.
Malformed/unsupported replies, duplicate fields, wrong request ID, unexpected
notifications and ambiguous result/error refuse initialization. Adapter failures
use ADAPTER_PROTOCOL, ADAPTER_TIMEOUT or ADAPTER_REJECTED without raw payloads.
The exposed runtime platform fields are observations, not authority or event
coverage. Native scope/run binding is checked, no thread/turn is started, and
all adapter capability guarantees remain absent. Failed/cancelled connect records
loss where possible; successful hosts must stop explicitly to journal their final
boundary. No stored PID, reply or environment can reconstruct a process permit.
Envelope version remains 0.1.0; that increment used database schema 11, superseded
by schema 12 for lifecycle observations and schema 13 for passive/cancellation
metadata above. See ADR-028–030.

Adapters report capabilities: lifecycle capture, exact event outcomes, interrupt request, verified quiescence, process containment, mutation gating, native session resume, environment export. Coordinator checks required capabilities per transfer plan; lack of a capability cannot be patched with an optimistic summary. Version negotiation rejects unknown critical protocol changes. Optional session format conversion can enhance continuity but does not substitute for snapshots, lease state or evidence.

## 12. Contract tests

Test every invariant under randomized scheduling, duplicated requests, crashes at each persisted state, stale epochs, hidden child processes, interrupt mid-external action, corrupted CAS blobs and divergent worktree. A test is successful only if the system either safely resumes or accurately refuses unsafe continuation. See `TESTING.md`.
