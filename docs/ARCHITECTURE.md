# Baton Architecture — Local First, Distributed Later

**Status:** Proposed design. All interfaces are conceptual until implemented.

Implementation notes: M1 supplies the experimental SQLite metadata engine;
M2 supplies `baton-checkpoint` and integrates artifact publication into the store.
Both remain disabled by default and do not implement execution authority. See
`M1-IMPLEMENTATION.md`, `M2-IMPLEMENTATION.md`, and ADR-017/018 for the implemented
contracts and limitations; the remaining modules below are still proposed.

M3 adds `baton-supervisor`, durable run/effect records in the store, fresh-only
launch permits and OS-specific process controls. It remains experimental, uses
trusted embedding inputs and has no agent-facing shell endpoint. Windows job
membership is queried independently of root exit; Unix groups cannot prove
escaped descendants absent. See `M3-IMPLEMENTATION.md` and ADR-019.

M4 currently adds `baton-platform` for checked local privacy/OS identity and
user-scoped IPC, and `baton-cli` for one `baton` executable with an opt-in daemon
subcommand. The daemon serializes store operations and derives the actor from
its exclusive OS-owned scope. Clients cannot submit supervisor facts or actor
bootstrap requests. Transfer coordination remains in progress; see ADR-020 and
`M4-IMPLEMENTATION.md`.

M5 adds the experimental `baton-adapter-codex` Rust library. The first implemented
slice uses the existing audited Session to own a piped App Server connection;
it performs only initialize/initialized, with bounded asynchronous I/O and exact
reply correlation. LaunchSpec fingerprints explicit transport/environment and
keeps inherited environment cleared. Native pipe ownership is transferred once;
failure/cancellation terminates the owned scope and records loss where possible.
No source turn/event capture, adapter capability proof or CLI launch command is
established by this handshake. See ADR-028 and M5-IMPLEMENTATION.md.

The later M5 source-admission probe is test-only: it runs the native vendor client
against a temporary loopback Responses fixture to inspect advertised tools under
restricted configuration. It does not add a product HTTP service or use that
inventory as authority. The strict candidate still exposes request_user_input;
initially lacked callback coverage. Schema 15 adds negative callback handling;
schema 16 adds an opt-in create-only callback backed by native host admission.
Actual vendor registration/source work remains missing. Source grants and
production transfer remain disabled. See ADR-032–035.

Schema 17 adds explicit experimental initialization and audited registration of
the fixed file tool in an ephemeral read-only thread. It does not start a turn.
The bounded pipe operation consumes fresh native admission; a separate vendor
startup effect stays uncertain and blocks later writes until validated policy/
resource evidence exists. source_thread and announced_thread retain correlated
reply/notification facts separately, with task-event hash and read-time checks.
Native registration through Source is tested without a model request; complete
source work and production transfer remain disabled. See ADR-036.

Schema 18 adds post-transition execution state hashes. All execution observations
are bound to task-journal hashes; reads check current run summaries against the
latest protected digest and observed-run retry results against their historical
digests. Empty journals permit only pending lifecycle data. The earlier thread-only
summary comparisons are subsumed by this check. These are integrity checks, not
resource reconciliation or source authority. See ADR-037.

Schema 19 adds a config/read rejection gate inside explicit file-thread registration.
Fresh native host admission covers its bounded pipe operation; only response
fingerprint, request identity and a services-present summary are retained. Reported
MCP, hook, plugin, marketplace or notify entries reject before thread/start,
including disabled entries. No reported entries is not isolation proof; the later
Unknown startup effect remains uncertain. See ADR-038.

Native launch-v2 fingerprints also include the streamed main-executable digest.
LaunchSpec holds the checked file; ProcessTree rechecks path identity and content
before spawn and retains the handle. Windows excludes ordinary writes/replacement
for that lifetime. This is a prerequisite for a version-tested startup profile,
not dependency attestation, complete path-race protection or isolation. Database
schema remains 19 and historical records are not upgraded. See ADR-039.

The M5 native containment tracer uses an externally supplied Microsoft MXC
executor under the existing owned native job, with a fixed benign Rust workload.
It does not add a production SDK/backend dependency or weaken the startup block.
Root exit status is retained separately from owned-job drain so a failed executor
cannot masquerade as successful policy enforcement. See ADR-040/M5-NC01–04.

## 1. Architectural thesis

Baton consists of a **native process supervisor and coordinator**, **immutable checkpoint storage**, **agent-specific adapters**, and **optional MCP/plugin frontends**. All authoritative safety decisions happen in the coordinator; agents and plugins are untrusted callers. Codex and Claude never exchange privileged ownership messages directly.

### Conceptual data flow

- **Codex or Claude adapter** -> structured, capability-labeled observations -> **event journal**.
- **Process supervisor** -> activity and quiescence evidence -> **checkpoint coordinator**.
- **Git worktree + file capture** -> hashed immutable blobs/manifests -> **CAS**.
- **Coordinator** -> transactional task state/lease/epoch -> **SQLite**.
- **Continuation compiler** -> bounded packet plus immutable evidence links -> **receiver adapter**.
- **CLI / MCP** -> authenticated, scoped requests -> **coordinator**. MCP is not the source of truth.
- **Future team client** -> encrypted sync -> **team coordinator (PostgreSQL + blob store)**.

## 2. Recommended stack and module boundaries

| Component | Suggested technology | Owned responsibilities |
|---|---|---|
| `baton-cli` | Rust, Clap | Human/JSON commands, init, doctor, start, transfer, inspect, recover |
| `batond` | Rust, Tokio, Axum | Local daemon, IPC lifecycle, request authorization, metrics, orchestration |
| `baton-protocol` | Rust, Serde | Versioned task/event/manifest types, state machine, capability contracts |
| `baton-store` | SQLite WAL + immutable CAS | Metadata transactions, event journal, blobs, hashes, GC |
| `baton-platform` | Rust + OS APIs | Private state, scoped IPC, opaque native process controls and drain proof primitives |
| `baton-supervisor` | Rust | Own live scopes, bind native observations, interrupt and drain managed runs |
| `baton-git` | Git CLI and worktrees | Base commit, staging semantics, clean receiver checkout, source-safe capture |
| `baton-checkpoint` | BLAKE3 + zstd | Incremental snapshot, integrity, policy-based capture, restore |
| `baton-context` | Deterministic evidence indexing | Structured continuation packet and evidence retrieval |
| `baton-adapter-codex` | App Server client | Start/watch/interrupt Codex, approvals, lifecycle event normalization |
| `baton-adapter-claude` | Agent SDK/CLI, hooks | Manage receiver and observe Claude lifecycle; guard tool entry points |
| `baton-mcp` | Local stdio MCP gateway | Authenticated task operations and evidence tools, forwarding to daemon |
| `baton-sync` (R2) | HTTP/gRPC + object store | Content sync, auth, remote team coordination and resume |

**Implementation choice:** a single Rust workspace and binary distribution initially. Use a subcommand to launch local stdio MCP gateway backed by the resident daemon. Do not create separate servers/microservices merely for modularity.

## 3. Trust and isolation boundaries

1. **Agent's prompt/context** is untrusted and cannot mint ownership epochs.
2. **MCP requests** are treated as untrusted inputs; validate task permission, epoch, parameters, and intended side effect.
3. **Hooks** are observational/advisory unless proven blocking on the installed runtime. They do not contain authority logic.
4. **Supervisor** owns the managed child-process tree, but cannot assume authority over unrelated user processes or external services.
5. **Local daemon** listens on user-scoped IPC (named pipe on Windows, Unix domain socket on Unix), not a public interface.
6. **Snapshot store** has strictly limited path scope; symlinks, hardlinks, submodules, junctions and traversal attempts cannot escape it.
7. **R2 coordinator** is the source of cross-host epoch authority, requiring authenticated writes and encryption in transit and at rest.

## 4. Storage model (conceptual entities)

- **Repository:** repo ID, canonical filesystem identity, Git root, trust settings, capture policy, schema version.
- **Task:** opaque task ID, goal, scope, status, current ownership epoch, current checkpoint pointer, version.
- **Actor/Agent:** human identity (local OS principal initially), adapter ID/version, permissions, capabilities.
- **Lease:** lease ID, epoch, owner, acquisition/renewal/expiration, state, fencing evidence.
- **Event:** monotonically increasing per-task event sequence, ID, causal parent, started/completed timestamp, event type, source tool, outcome, source event ID, evidence hash.
- **Effect:** operation ID, expected effect classification, external target if known, idempotency key if supported, observed completion/ambiguity and reconciliation requirement.
- **Checkpoint:** checkpoint ID, parent checkpoint, immutable manifest root hash, event high-watermark, originating epoch, policy and verification result.
- **Transfer:** transfer ID/idempotency key, source/receiver identity, checkpoint ID, expected epoch, prepared/validated/committed status, error reasons.
- **Context entry:** evidence reference, content kind (fact/agent claim/inference), freshness, confidence/relevance only for recommendations, never authority.
- **Artifact reference:** content hash, size, encoding, sensitivity classification, encryption key ID, retention scope.

### Local transaction policy

Authoritative changes to tasks, leases, transfers and event cursors occur in SQLite write transactions. File blobs are staged immutably first; metadata publishes a **root manifest pointer** only after verified durable writes. A crash before metadata commit may leave orphan blobs that GC can later delete. A crash after commit must always resolve to existing, validated blobs. Specify exact fsync/rename strategy per OS and filesystem; **atomic rename and SQLite commits are not a single transaction**.

A checkpoint must not be announced VERIFIED unless the manifest's recorded content and capture policy have been validated. Migrations should run transactionally and retain backup/recovery semantics.

## 5. Snapshot semantics

Represent a snapshot as Git base + structured delta and manifests, not a plain `git diff` alone.

- Include **index/staging information**, tracked modifications, deletions, modes/permissions where portable, intended untracked files, and byte-exact file contents.
- Record ignored and excluded files as **declared gaps**, with per-category counts and reasons; never silently treat an incomplete capture as complete.
- Detect submodules/LFS pointers, worktree metadata, symlinks/junctions, case collisions, file locks, large blobs, sparse checkout and line-ending behaviors.
- Protect secrets and user data with default exclusions, warnings, explicit opt-in inclusion, and local-only sensitive classification.
- Reuse immutable blobs by hash; compress selectively to avoid spending CPU on tiny or already-compressed content.
- Do not capture external databases, package caches or process memory as if they were Git files; describe them in environment/effect manifests.
- Restore to fresh receiver worktree, verify hashes and staging state, then expose environment gaps. Do not overwrite an existing checkout.
- Align the event cursor with snapshot consistency: pause writes or detect divergence and retry; never combine tools from one epoch with files from a later unacknowledged epoch.

## 6. Supervisor details

- Launch agents through Baton to make safety capabilities explicit; unmanaged IDE sessions may only be imported in read-only/degraded mode.
- Track direct children and descendants; use OS process groups or **Windows Job Objects** as applicable.
- On transfer request, stop new agent work, request graceful quiescence, record last safe event boundary, drain/observe descendant processes, then escalate as explicitly permitted.
- Graceful termination of agent turn is not proof that a background shell process stopped or external action completed. Log unresolved operations to effect ledger.
- A stale process may remain alive despite lease expiry. Task ownership authority and actual OS-level access must be separated in diagnostics.
- Adopt narrow workspace scope and controlled execution paths for enforceable guarantees; refuse unsafe automatic transfers when unable to fence writes.

## 7. Context compiler

Input: authoritative task/checkpoint manifests, event log, tests/errors, explicit user goals, agent-reported decisions, and optional inferred summaries.

Output: **continuation packet** containing task ID/epoch, intended goal, latest verified boundary, changed-file manifest, outstanding verified test failures, unresolved effects, known environment mismatches, compact event citations and on-demand evidence tool names.

Rules: prioritize explicit requirements and recent relevant events, cap context payload by configurable budget, never omit safety warnings to meet budget, label facts/claims/inferences, never treat source code or previous agent transcript as instructions with higher priority than current user/developer instructions. All descriptions must link back to event or artifact IDs. Do not require a vector store; deterministic retrieval first.

## 8. IPC, APIs and versioning

- **CLI <-> daemon:** versioned local IPC over OS user-scoped sockets/pipes, structured error codes, privilege check.
- **MCP <-> daemon:** MCP server forwards validated requests, signs/attaches limited task capability if necessary; separate read from mutation tools. No raw filesystem or arbitrary shell tools in the Baton MCP surface.
- **Adapter <-> daemon:** structured event transport with sequence IDs and acknowledgments; duplicate events deduplicated, missing gaps detectable.
- **R2 client <-> server:** TLS, authenticated actors, PostgreSQL transactional CAS and fenced leases; immutable blobs uploaded with manifest commit after hashes validated.
- **Schema:** semantic version on envelopes and capability negotiation. Unknown critical fields reject; optional fields may be ignored with provenance. Explicit migrators needed for old checkpoints.

## 9. Failure domain and recovery

M5 schema 12 adds bounded adapter reports and gap markers to the existing
execution journal, rather than introducing a separate vendor database. Task-event
hashes bind new execution records; reads and exact retries verify them. Stream
correlation is derived from immutable entries. The Rust normalizer stores reported
IDs/status/exit code, omits raw text, and assigns run-scoped deterministic operation
and effect IDs. Unknown tool effects remain uncertain after success. Missing
predecessors and lost supervision remain gaps; turn completion grants no native
drain proof. Unsupported messages stop the owned capture scope. This is partial
lifecycle coverage with production transfer closed. See ADR-029.

Schema 13 uses the same journal for passive item metadata and cancel-only approval
control. HostReported cancellation intent is committed before native pipe I/O;
HostReported written evidence commits after flush. AdapterObserved resolution
correlates through the saved intent. Typed RPC IDs and phase-specific run-scoped
keys prevent rebinding or resolved-ID reuse. Passive content is discarded. No new
database, approval service or execution endpoint is introduced; tool effects remain
unsettled and production transfer stays closed. See ADR-030.

Schema 14 extends the same run journal with turn-interrupt planned/written/reply
phases. A deterministic request ID binds the active observed turn to the native
transport's authenticated run. Intent sets STOPPING before I/O; written evidence
prevents another send on live retry. The capture loop correlates the empty reply
through durable intent while accepting supported interleaved notifications. It
does not use a reply or terminal turn to mark native drain or settle effects.
Ambiguous intent cannot replay, and failed transports are torn down using the
existing guard. No second RPC runtime or process-adoption path is added. See ADR-031.

M4's current durable transfer slice stores one pending intent per task in schema
11, with append-only record versions linked by hash to task events. Requests,
preflight and abort use the existing authenticated/idempotent SQLite transaction.
The source task reservation stays separate from transfer intent. No native proof
or activation authority is reconstructed from transfer JSON on restart. The
independent policy model exercises transition rules with synthetic facts;
production boundary proofs remain missing. Staging and ownership CAS are guarded
by engine checks; local process activation remains disabled.

Artifact restore uses two SQLite transactions around filesystem work: immutable
intent first, then the existing authenticated result transaction. Intent reserves
the complete request fingerprint and operation ID globally. A private immutable
completion receipt allows recovery after files validate but before the final
result commits. Recovery rechecks receipt binding and actual receiver content;
it never adopts an unrecognized target or reruns filesystem restoration over it.
This is a forensic artifact path, not PREPARED transfer or activation authority.

Native scope binding and observed drain are immutable schema-7 records. The
platform process object owns the actual handle/group and a fresh scope ID;
supervisor retains ownership for its lifetime. Store accepts native evidence
through typed borrowed objects, separate from ordinary observation data. Native
records, terminal execution, task hashes, audit and request results commit in one
transaction. Reads check the corresponding journal evidence. These primitives
do not provide a complete safe boundary or reconstitute live control on restart.

Schema 8 stores receiver stage intent inside its transfer record, with unique
destination and operation identities. The reserved operation fingerprint is
checked across all actions and forensic restore intent. Filesystem completion
uses the existing immutable receipt; validation, stage publication and metadata
ownership CAS are transactional. Source revalidation recaptures into private CAS
for hash comparison without modifying the source worktree; changed captures can
leave unreferenced private objects for normal retention/GC. Committed task reads
cross-check the audited transfer's owner/epoch. The unit-only boundary fixture
never compiles into the daemon library.

Schema 9 records a receiver activation's pending RunRecord in the transfer, binds
it to one task/epoch run, and checks immutable launch/binding fields on read.
Trusted embedding supplies a native LaunchSpec; its fingerprint and capabilities
participate in a separate idempotency domain. A fresh committed transaction can
issue one opaque LaunchPermit, consumed by the existing supervisor. Cached data
cannot recreate it. Native launch stays ACTIVATING/BLOCKED until a host-bound
readiness acknowledgement. The CLI cannot supply a launch descriptor, and production
boundary checks still prevent this path from issuing a permit. See ADR-025.

Schema 10 stores normalized receiver readiness inside the transfer and enforces
unique session IDs. The platform produces a borrowed sampled-running-root proof;
Session supplies it alongside the adapter acknowledgement. Store checks the
actual native binding, committed grant, run state and engine requirements, then
journals ACTIVE metadata atomically. The acknowledgement is not a validated
vendor session: quality stays BLOCKED/RECEIVER_ADAPTER_UNVALIDATED. Errors stop
the owned Session; terminal native evidence precedes the recovery journal entry.
There is no JSON proof input, process adoption or persisted launch replay. See
ADR-026 for the implemented boundary and fixture-only positive coverage.

Schema 11 separates the current committed grant from a pending transfer slot,
allowing an ACTIVE receiver to become the next QUIESCING source. Current workspace
resolution follows that grant's completed receiver stage, receipt format 2 and
canonical Git identity; it does not use a caller-selected source path. The common
Git directory remains bound to enrollment. A narrow receipt-bound capture API
can read that receiver inside the vault; ordinary vault-overlapping capture is
still refused. Each committed epoch has a unique grant. Old stores and legacy
receipts are refused without rewriting. See ADR-027.

The native host can reserve a classified tool with a fresh-only ToolPermit, then
consume it under current authority. ToolReserved is durable admission intent;
ToolStarted/Finished are observed facts. Native scope, canonical launch cwd, run,
actor, epoch, lease and sequence are checked together. This API admits only
ReadOnly/LocalReversible operations in task scope `.`; trusted host policy must
classify and constrain the operation. It supplies no generic shell endpoint or
OS sandbox. Saved metadata cannot recreate permits. Late activity during
QUIESCING is recorded and forces recovery. Unit fixtures exercise this path with
a typed synthetic child's known local write; production transfer proofs stay closed.

M5 now provides a concrete trusted-host create-only file primitive through Session.
The launch cwd is held as a directory capability before spawn; a fresh native
tool reservation binds operation/run/name/content. Flat names and 16 KiB content
are bounded, existing files are preserved, and sync/readback precede observed
completion. Errors preserve partial files and stop the managed scope; retries
cannot reconstruct a permit. Schema 16 adds the opt-in callback below; no CLI
command exposes it. cap-std 4.0.3 supplies relative access. See ADR-034/035.

Schema 15 records callback refusal plans, written negative results and optional
vendor resolution in the existing execution journal. Shared server-request phase
keys prevent approvals/callbacks rebinding an RPC ID. Raw-frame fingerprints bind
omitted question/argument contents without storing them. User input stops the
scope without an answer; dynamic requests get only success:false. Dynamic item
lifecycle is ClientToolCall metadata until a separately admitted host action exists.
No effect, tool worker, source writer flag or positive admission is inferred. See
ADR-033 for compatibility and native raw-protocol test limits.

Schema 16 adds execution callback metadata and host opt-in for
baton_create_file_v1. Source journals intent, invokes the Session operation,
then sends a digest-only success after host completion commits. Written/Resolved
remain distinct from file completion. Shared request IDs prevent rebinding or
replay across callbacks/approvals. Default capture keeps refusing; no tool is
registered with the real vendor and no source grant is inferred. See ADR-035.

- **Daemon crash:** replay committed database state; scan staged transfers, orphan manifests, active child PIDs and outstanding effects; fail closed if uncertain.
- **Agent crash:** preserve last checkpoint; do not claim latest in-memory events persisted; detect unknown active subprocesses.
- **Git changes after staging:** abort/mark degraded; hash match before ownership commit.
- **Stale epoch:** reject coordinator/API mutations; managed process fencing required beyond database rejection.
- **Disk full or corruption:** no checkpoint acknowledgement, no ownership transition; retain last validated checkpoint.
- **R2 partition:** no automatic double-writer; unavailable shared authority means read-only or BLOCKED, not lease-based takeover without isolation proof.

## 10. Performance design principles

Measure before optimizing: event observation latency, checkpoint creation under varying file change sizes, restore time, context compiler wall time and redundant receiver tool calls. Avoid blocking on compression in hot event paths; batch journal writes carefully without compromising durability acknowledgment. Prefer file-level CAS first, add chunking only if measured benefits justify complexity. No Kafka/Redis/embedding service for R0.

See `PROTOCOL.md` for exact transfer lifecycle and invariants; `SECURITY.md` for safety boundaries and threat model.
