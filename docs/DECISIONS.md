# Baton — Architecture Decision Log

This document records product/engineering **proposals accepted as the implementation baseline**, not immutable facts. When a validated need requires change, add a superseding decision (date, alternatives, impacts, rollback plan, affected `FR`/`INV`/tests) rather than silently editing historical intent.

| ID | Status | Decision | Rationale | Tradeoff/revisit trigger |
|---|---|---|---|---|
| ADR-001 | Proposed baseline | Native local-first CLI + daemon is the authoritative engine | MCP/plugin cannot guarantee process supervision or durable snapshots | Revisit if a host offers audited equivalent control |
| ADR-002 | Proposed baseline | Rust/Tokio/SQLite WAL for R0 core; Axum only where HTTP needed | Cross-platform performance, deterministic transactions, low idle footprint | Benchmark portability, complexity and real build time |
| ADR-003 | Proposed baseline | Git worktrees + immutable CAS with BLAKE3/zstd | Non-destructive restoration, deduplication, integrity | Evaluate performance on very large repos and binary-heavy data |
| ADR-004 | Proposed baseline | Transfer only from verified quiescent source boundaries | Prevent duplication, half-writes and uncontrolled child processes | Stronger sandboxed execution may safely expand boundaries |
| ADR-005 | Proposed baseline | Coordinator-issued fencing epochs plus lease heartbeat | Stale commands must not regain authority; timeouts alone are insufficient | Distributed strong fencing needs resource-level enforcement |
| ADR-006 | Proposed baseline | Separate source checkpoint publication from owner transfer transaction | Blob store and DB are not one atomic store; recovery remains idempotent | More robust storage transactions if proven portable |
| ADR-007 | Proposed baseline | Three statuses: VERIFIED, DEGRADED, BLOCKED | Avoid false promise of fidelity; explain incomplete context | Refine status reasons, never remove uncertainty |
| ADR-008 | Proposed baseline | Codex App Server source; Claude SDK/CLI + hooks receiver | Official lifecycle and events; no private session scraping | Adapt after actual compatibility tests |
| ADR-009 | Proposed baseline | MCP is optional control/evidence interface; plugins are onboarding wrappers | Clear authority boundaries and host independence | New plugin capabilities may improve setup, not replace core |
| ADR-010 | Proposed baseline | Provenance: observed facts vs agent claims vs inferences | Prevent false certainty and prompt-induced authority escalation | Evolve structured context ranking after evaluations |
| ADR-011 | Proposed baseline | Cloud/team work waits for correct R0 local demo | Distributed failure modes can obscure core correctness | R2 starts only after local fault-injection gate |
| ADR-012 | Proposed baseline | No unattended replay of uncertain external mutations | Arbitrary exactly-once semantics cannot be promised | Add resource-specific reconciliation/idempotency integrations |
| ADR-013 | Proposed baseline | No raw private agent reasoning/state transfer promise | Neither cross-provider hidden state nor deterministic future generation is exposed | Native same-agent resume can be an opt-in enhancement |
| ADR-014 | Proposed baseline | Per-repo opt-in enrollment and no default artifact upload | Captured commands/transcripts/files may contain sensitive data | Revisit only with informed consent and secure policy controls |
| ADR-015 | Proposed baseline | Native installer primary; Codex/Claude plugin discovery secondary | Correct user mental model and trust during setup | Update distribution choices as vendor marketplaces evolve |

## Template for new decision entries

**ADR-XXX — Title**

- **Date / status:**
- **Context and problem:**
- **Options considered:**
- **Decision and why:**
- **Safety/invariant implications:**
- **Affected requirements and tests:**
- **Migration/rollback plan:**
- **Open risks and when to revisit:**

## Unresolved questions for later milestones

- R1: default retention duration, user-level encryption strategy, binary signing and distribution channels.
- R1: precise hook bypass coverage in each supported release and whether any mode can be called enforced versus cooperative.
- R2: hosted vs self-hosted coordinator and corresponding auth/device identity model.
- R2: safe offline collaboration policy—default should be read-only if server authority cannot be established.
- R2: key revocation and artifact access after member removal.
- Long term: support for imported unmanaged sessions, external container snapshots, reverse transfers and editor UX.

## ADR-016 — Minimal M0 crates and strict initial schema

- **Date / status:** 22 October 2026; accepted for the M0 implementation.
- **Context and problem:** M0 needs executable contracts and fixtures without
  creating placeholder daemons, persistence engines, or adapter claims.
- **Options considered:** scaffold every proposed architecture crate now; create
  only crates with implemented M0 behavior.
- **Decision and why:** create `baton-protocol` and test-only
  `baton-test-support`. Add other architecture crates when their milestones supply
  working behavior. Rust 2021, stable toolchain, checked-in Cargo lockfile; Serde
  for contracts, tempfile for owned test directories. Defer Tokio, SQLite, Axum,
  hashing, and compression until used. This refines sequencing, not the native
  architecture or safety baseline.
- **Schema policy:** initial diagnostic schema is exactly `0.1.0`. Missing or
  unsupported versions, unknown codes, and unknown fields reject. No optional
  extension fields or migration is implemented. Additive optional fields require
  a documented negotiated contract; incompatible changes need a new version and
  explicit migration policy. Cargo package and schema versions are independent.
- **OS policy:** no optional Cargo features in M0. Use Rust target `cfg` for OS
  differences; never let a build feature disable safety validation. Native
  process/IPC capabilities remain absent until their implementation and OS tests.
- **Safety/invariant implications:** no writer, ownership, checkpoint, or recovery
  engine exists. Recovery labels alone grant no authority. Diagnostic metadata
  contains no paths, command outputs, or user-supplied strings. Git fixture
  commands are for trusted tests only, not an agent-facing execution interface.
- **Affected requirements and tests:** foundations for FR-01/09/13/19, NFR-08/09;
  INV-05/08/09 contract support only; M0-001 through M0-007. No release invariant
  is claimed proven.
- **Migration/rollback plan:** no user state exists to migrate. Future schemas
  must reject unsupported inputs until an explicit compatible reader/migrator
  exists. Crates may move without changing wire codes.
- **Open risks and when to revisit:** native macOS/Linux and Windows MSVC evidence
  is pending. Keep the M0 exit gate open; do not substitute cross-compilation for
  native runtime tests. Revisit schema evolution when M1 introduces records.

## ADR-017 — Experimental M1 metadata before execution authority

- **Date / status:** 22 October 2026; accepted for this development increment.
- **Context and tension:** the user authorized the next part after the M0 report,
  with native three-OS CI still pending. M1 also references T-001/T-002/T-008,
  whose full definitions include transfer/activation behavior that cannot be
  implemented safely before M3 supervision and M4 two-phase transfer. Initial
  task ownership states were not specified before ACTIVE, which requires an
  established supervisor. OS IPC authentication does not exist yet.
- **Options considered:** block all local development awaiting external CI;
  claim complete ownership/transfer safety from database leases alone; implement
  an explicitly non-executing metadata slice behind an experimental feature.
- **Decision:** proceed with the user-authorized development increment, keeping
  M0's validation gate pending. Add UNCLAIMED/RESERVED/SUSPECT as metadata states
  preceding the BCTP execution lifecycle. Claim/cancel/reclaim exercise monotonic
  epoch CAS only for never-activated reservations. There is no ACTIVE state,
  force takeover, execution capability, or automatic expiry recovery in M1.
  Full two-phase transfer remains deferred, not simulated as success.
- **Authentication boundary:** trusted embedding code alone bootstraps opaque
  in-process actors. Requests must match that actor's hashed secret and task ACL;
  a supplied actor UUID alone is insufficient. This is not OS principal binding
  or daemon IPC authentication. Actor capabilities are not serializable and have
  no secure restart persistence yet. Those gaps and unvalidated Windows state
  ACLs require `baton-store/experimental`, disabled by default. CI explicitly
  enables it for tests and separately checks the disabled build.
- **Storage choice:** bundled rusqlite/SQLite, WAL + FULL + foreign keys checked
  per connection, BEGIN IMMEDIATE for mutations, read transactions for consistent
  cursors. SQLite application ID and user_version=1 identify the first database;
  unknown/foreign versions refuse without auto-migration. Records use schema
  0.1.0 with strict decoding. New pre-release error codes reject on older readers;
  existing diagnostic codes/format remain unchanged. No compatibility with an
  older task database is claimed (M0 had none).
- **Idempotency/audit:** a global opaque operation ID binds actor and full
  canonical request fingerprint. Successful and logical denied results persist
  with state/events in one transaction. Changed input cannot reuse a key. Exact
  retries return historical responses, never a fresh grant or renewed expiry;
  each new mutation must still validate current ownership. I/O/busy/corruption
  errors remain unacknowledged and may retry the same ID. Task event cursors are
  committed with journal rows, allowing middle and tail loss detection.
- **Safety implications:** no guarantees weakened in INV-01/02/04/05/06/07/09;
  these guarantees are bounded to the implemented paths. SQLite cannot fence
  filesystem writers. SUSPECT is sticky and preserves the old owner/epoch.
  Administrative task access changes do not enable execution.
- **Affected requirements/tests:** FR-01/02/04 and metadata portions of FR-07/08;
  INV-01/07, conservative INV-06; M1-001–011, M1-P01/P02 and
  T-001-M1/T-002-M1/T-008-M1. Complete transfer variants remain M4 tests.
- **Migration/rollback:** no automatic schema migration or downgrade. Existing
  foreign/future stores are refused. Disabling the feature leaves data intact.
  Future ACTIVE states must remain ineligible for CancelReservation and require
  verified fencing before any new owner. No user database was created here.
- **Open risks/revisit:** finish native three-OS validation, OS-bound
  authentication, secure capability recovery, and directory ACL validation
  before production exposure. M3/M4 must supersede the reservation-only boundary.

## ADR-018 — M2 forensic artifacts before safe-boundary supervision

- **Date / status:** 22 October 2026; accepted for the experimental M2 slice.
- **Context:** M2 requires immutable workspace capture and restoration before M3
  can prove process quiescence. The user authorized proceeding, while native M0
  builds and M1 identity/ACL gates remain pending. A matching file hash cannot
  prove absence of a live writer or external effects. Windows directory-entry
  durability is not proven by the available implementation.
- **Options considered:** claim VERIFIED from matching files alone; block all
  artifact work until supervision; build an explicitly non-executing forensic
  engine with no production exposure. Choose the last, preserving all gates.
- **Implemented boundaries:** `baton-checkpoint` owns local Git capture, CAS,
  path/policy validation and isolated restoration. `baton-store` owns authenticated
  requests, epoch/lease checks, transactional root publication and audit. The
  store's experimental feature enables the low-level library; neither has a CLI,
  daemon, MCP or network interface. Low-level Vault APIs are trusted embedding
  primitives, not agent-facing authority operations.
- **Snapshot decision:** two complete scans compare supported byte content,
  index identity, HEAD and declared gaps. Keep separate index/worktree blobs and
  original Git index OIDs; rehash source blobs through Git before capture and
  verify receiver OIDs. This detects observed divergence but not ABA writes or
  quiescence. Every continuation outcome is BLOCKED; no PREPARED/ACTIVE state is
  introduced. Supported metadata is Git regular/executable mode and Unix working
  execute bits; full ACLs, ownership, xattrs and timestamps are out of scope.
- **Policy decision:** local-default-v1 includes ordinary tracked files, explicit
  exact-path nonignored untracked files, and no known sensitive artifacts. Exclude
  entire versions rather than redact bytes and claim byte fidelity. Known secrets,
  ignored/unselected files, links, submodules, LFS and size gaps are explicit.
  Sensitive inclusion, raw sessions and export remain unavailable pending
  encryption/privacy work. This does not erase secrets already in Git history.
- **Storage decision:** BLAKE3 keys of uncompressed data, zstd only for beneficial
  compression, strict bounded decoding, explicit staging manifests, create-new
  flushed temporary files and no-clobber hard links. Unix syncs containing
  directories; Windows records DURABILITY_UNPROVEN and cannot claim a durable
  VERIFIED checkpoint. SQLite FULL commits only after current artifact reads and
  hash validation. Orphan files are allowed on interruption, never partial root
  acknowledgements. A hard-link-capable private local filesystem is required.
- **Restore decision:** reserve a UUID destination inside the owned restore root,
  then `git worktree add --no-checkout --detach --lock`. Materialize included
  bytes only and reconstruct the index via plumbing, with hooks/filters disabled
  in those paths. Preserve source/index and existing targets. Preserve partial
  targets after failure; do not replay restoration over them or use --force.
- **Schema decision:** new stores use database user_version=2, with immutable
  checkpoint roots; M1 schema-1 stores refuse with UNSUPPORTED_SCHEMA. Do not
  silently mutate an old store or claim a backup/migration implementation exists.
  New manifests are schema 0.1.0; they did not exist in M1. Existing M1 task wire
  records remain unchanged, and new nullable file-state fields require presence.
- **GC decision:** retain every published root across all tasks/parents and every
  unpublished/staged manifest. Validate roots before collecting only unreferenced
  blob names. Do not implement live-checkpoint/staging retention deletion yet.
  Store GC is serialized with publication and requires creator plus current lease.
- **Affected requirements/tests:** FR-04/05/09/16, INV-03/05/07/09, T-004/T-005
  subcases, M2-P01/P02 and M2 matrix/storage tests. No complete M2/R0 release gate.
- **Rollback/migration:** disable the experimental features without deleting
  state. Preserve unrecognized stores, orphan staging and failed receivers for
  explicit reconciliation. No automatic downgrade or migration is supplied.
- **Remaining gates:** M3 safe boundaries, M4 two-phase ownership, native OS CI,
  Windows ACL/directory durability, encryption, full fixture and I/O/power-loss
  fault coverage, representative performance and efficient large-repo capture.

## ADR-019 — Durable launch intent and bounded native supervision

- **Date / status:** 22 October 2026; accepted for the experimental M3 slice.
- **Context:** the complete-project goal authorizes the next roadmap milestone,
  while M0–M2 release gates remain open. Process exit, interrupt acknowledgement,
  local child containment and external-effect safety are independent facts.
- **Decision:** add `baton-supervisor` and store run/effect journals. Commit intent
  before issuing an opaque, nonserializable, noncloneable launch permit. Only the
  fresh request obtains a permit; cached replies and crash recovery never do.
  Validate actor, lease, epoch, pending record and exact parameter fingerprint
  before launch. No process activation RPC or recovery shell replay is added.
- **Alternatives:** rely on a PID/lease or wrapper wait result; implement raw OS
  calls in workspace code; use safe OS wrappers with independent job membership
  queries. Choose safe wrappers, retaining `unsafe_code = forbid` in the workspace.
  Pin process-wrap 10.0.1 and win32job 2.0.3 after reading their source. This costs
  additional Windows dependencies but avoids private unsafe process APIs.
- **Windows:** process-wrap creates suspended and resumes after job assignment.
  A post-spawn hook assigns a separate unnamed non-inherited kill-on-close job
  before that resume; its membership query includes nested jobs. No breakaway
  flags are enabled. The wrapper's job supplies termination; the separately owned
  job supplies membership and supervisor-crash teardown. Do not infer emptiness
  from root exit, completion-port traffic or interrupt acknowledgement. Query
  overflow (>approximately 1,024 processes) or control failures block.
- **Unix:** process groups provide cooperative termination, not proof against
  setsid/reparenting/escaped descendants or supervisor hard kill. Root exit yields
  Unknown and RECOVERY_REQUIRED. Never signal a group after reaping its leader;
  this avoids acting on a potentially reused group ID. Strong automatic Unix
  continuation remains disabled pending a validated containment backend.
- **Events/effects:** append normalized, bounded, source-sequenced observations
  with task/actor/run/epoch/operation binding and coordinator timestamps. Gaps are
  sticky, duplicate operations replay metadata only. Interrupted/incomplete,
  unknown or external actions stay uncertain. Hash-only reconciliation is
  explicitly refused until resource-specific evidence validation exists. Adapter
  capability booleans are declarations, not installed-runtime safety proof.
  Raw output capture is disabled; evidence-unavailable is explicit. No hidden
  model state or completed test evidence is inferred from process exit.
- **Authority limits:** these are trusted embedding APIs. Session separates
  adapter input from native process observations; Store's low-level observation
  primitive must not be exposed to agents or IPC clients. M4 must introduce the
  complete authority boundary and verify live proof, rather than trust serialized
  records. Process jobs are not filesystem/network sandboxes or external fences.
  Only explicitly supplied executable/argv/cwd run; environment is cleared,
  stdio is discarded and Windows processes have no visible window.
- **Storage/rollback:** database schema 3 adds runs and immutable execution events;
  schema 1/2 refuse without migration. For this increment each task can reserve
  one run; metadata cancellation is forbidden once a run exists. Lost launch
  response or supervisor identity requires explicit reconciliation, never PID
  adoption. Disable experimental features to roll back, preserving data.
- **Requirements/tests:** FR-03/04/06/12/14/15, INV-04/06/07/09; M3-P01–P03,
  M3-S01–S04, M3-N02/N04–N07, T-006/T-007 subcases, before/after intent-commit
  kills, child survival and supervisor hard kill. Full M3/R0 gate remains open.
- **Revisit:** native Unix containment and all OS evidence; secure OS identity,
  durable credentials and Windows ACLs; journaled control recovery; complete vendor
  event streams and negotiated capabilities; resource reconcilers; M4 activation.

## ADR-020 — Owner-scoped local control without saved bearer credentials

- **Date / status:** 22 October 2026; accepted for M4's local-entrypoint increment.
- **Context:** a usable CLI/daemon needs identity across restart, while M1 actor
  capabilities were intentionally in memory only. Exposing trusted store methods
  directly would let clients fabricate native process observations. The Windows
  temp parent on this host grants another group read access, so default inherited
  permissions alone are insufficient for private state.
- **Decision:** introduce `baton-platform` privacy/identity/IPC primitives and
  `baton-cli`, producing one experimental `baton` binary with a daemon subcommand.
  Build this boundary before exposing transfer grants. No frontend, MCP or cloud
  component is added. Actual transfer coordination remains unfinished.
- **Privacy:** create only new directories below an owner-controlled parent that
  others cannot write. Read-only parent access is permitted while the new empty
  directory is protected; no data is written before validation. Windows installs
  a protected owner-only inheritable DACL using safe wrappers; Unix uses 0700 and
  0600. Existing broad ACLs/modes are refused, never silently repaired. Reject
  linked state leaves, reparse points, capability/database hardlinks and UNC state.
  Permit only root-owned Unix ancestor aliases whose parent is root-owned and
  non-writable by others; the approved state leaf still requires owner/privacy
  checks. Administrators/root/SYSTEM and same-user malware remain outside the
  privilege-isolation guarantee. Private filesystem access is not encryption.
- **Identity:** hold an exclusive OS file lock for daemon lifetime. Resolve a
  stable actor ID from the actual SID/effective UID in schema-4 SQLite state and
  rotate its random in-memory capability at bootstrap. Audit identity recovery
  and endpoint binding with coordinator-clock high-watermark timestamps. Save no
  bearer token or replayable process recipe. Existing unbound actors are not
  automatically adopted. Schema 1/2/3 refuse without migration.
- **Transport:** Windows uses an owner-only named-pipe DACL, remote-client refusal,
  identification-only client SQOS and a server-owner check through READ_CONTROL
  before transmission. Unix uses a private directory/socket and matching peer
  effective UID in both directions. Use a fresh session locator each boot; stale
  locator/socket presence never proves liveness and never triggers takeover.
  Compact Unix socket names use 64 bits of the full session UUID as a locator,
  not a credential. Exclusive locking plus no-overwrite bind refuses collisions.
  Existing stale sockets are not adopted or removed during crash recovery.
- **Protocol:** strict schema-0.1.0 envelopes bind operation ID and expected epoch;
  the server supplies actor identity. Supervisor/actor/retention methods are
  excluded from the local mutation allowlist. Local and trusted operation hashes
  use separate domains, including on cached replies; denials persist and audit.
  Frames, pages, connection count and I/O waits are bounded. Only unsent pipe-busy
  connection setup retries automatically. Lost responses retain uncertainty and
  require the original operation ID; generated resource IDs default to that ID.
- **Quality:** every current local response reports BLOCKED with
  EXECUTION_NOT_ENABLED, separately from metadata-operation success. Human errors
  include corrective actions; JSON retains versioned codes. Snapshot-specific
  limitations remain explicit. No process start or transfer activation endpoint
  exists, and serialized capability/process claims cannot grant execution.
- **Requirements/tests:** FR-01/02/04/09/12/13/16 and local FR-08 foundations;
  INV-02/03/05/07/09. M4-OS01–06, M4-IPC01/02, M4-ID01/02, M4-P01,
  M4-CLI01–04 and duplicate local-claim subcases. Native Windows GNU evidence;
  platform code type-checks for Linux and macOS, without native runtime claims.
- **Rollback / next work:** disable the experimental executable/features while
  preserving all state and user worktrees. Finish the two-phase transfer state
  machine and native-proof enforcement before enabling writers. Native OS runtime,
  cross-account abuse, artifact durability, disk-locality checks on other mounted
  filesystems, complete privacy/encryption and vendor coverage remain gates.

## ADR-021 — Transfer policy model and blocked durable preflight

- **Date/status:** 22 October 2026; accepted as an incremental M4 implementation,
  not an exit-gate waiver or a change to the protocol invariants.
- **Decision:** separate the pure transfer policy model from execution authority.
  Synthetic facts test ordering, receiver authorization, monotonic commit and
  explicit degraded acknowledgement. Neither the model nor serialized records
  mint launch permits. Production control exposes only request, preflight, read
  and abort until native fencing, complete events and adapter validation exist.
- **Storage:** schema 5 adds transfer records, one pending transfer per task and
  an append-only transition journal. Task evidence, state, audit and idempotent
  results commit in one SQLite transaction. Schema 1–4 is refused without edits;
  a safe migration is not implemented. This is a deliberate experimental storage
  compatibility break, not silent state adoption.
- **Authority:** QUIESCING records intent only. Pending transfer intent refuses
  new launch reservations, subsequent permit validation and reservation
  cancellation; it does not claim a running or concurrently spawning agent
  has stopped. Source heartbeat, observations and forensic capture remain usable.
  Preflight verifies checkpoint content/epoch/actor and records missing proof
  categories as BLOCKED. Abort requires source identity and current epoch,
  preserves ownership and never revives an expired reservation.
- **Limits:** no receiver staging, native proof seal, ownership commit or process
  activation is implemented by this increment. Fake policy success is not T-003
  end-to-end success. Native Unix, artifact durability and vendor gates remain.
- **Validation:** M4-TR01–05, M4-CLI05 and four transfer model tests trace to
  FR-08/09/12/13 and INV-02/07/08/09; the report records actual results.

## ADR-022 — Durable restore intent and bound completion receipts

- **Date/status:** 22 October 2026; accepted for M4 artifact staging. Transfer
  staging still requires PREPARED; this increment does not waive that gate.
- **Problem:** filesystem restoration cannot roll back with SQLite. A coordinator
  kill after successful file restoration but before recording the response left
  a complete worktree that an exact retry could not safely distinguish from an
  unrelated existing destination. Matching bytes alone are insufficient evidence.
- **Decision:** commit immutable restore intent before filesystem work. Bind the
  authenticated actor, task/epoch/lease, checkpoint, restore ID, operation ID and
  local/trusted request domain through the full request fingerprint. Reserve that
  operation ID across every action, including before a final result exists.
  Record RESTORE_RESERVED evidence and audit in the same intent transaction.
- **Completion:** after restoring into a create-new worktree and verifying it,
  publish a flushed, immutable schema-0.1.0 receipt in private engine storage.
  The receipt locator derives from restore ID; it is not a content-addressed
  artifact. An exact retry may reuse only its matching receipt after fresh
  manifest, repository identity, index and worktree checks. It never rewrites
  the existing destination. Missing receipts, edited receivers and conflicts
  preserve files and refuse reuse. Generic `Vault::restore` keeps its original
  no-existing-target contract.
- **Authority:** intent and receipts prove no process boundary and grant no
  writer authority. The request transaction still checks current actor, lease,
  epoch and expiry before filesystem work. Completed historical replies remain
  forensic and revalidate contents. Partial targets remain BLOCKED; no automatic
  cleanup, overwrite, source resume or ownership change is introduced.
- **Compatibility:** schema 6 adds immutable restore-intent rows and requires the
  new receipt directory in fresh vaults. Schema 1–5 is preserved and refused;
  migration is not implemented. Windows directory durability remains unproven,
  and all continuation reports retain BLOCKED. Power-loss safety is not inferred
  from process-kill tests.
- **Validation:** M4-RS01–03 cover request binding, receiver edits, unrecognized
  targets, actual kills before/after intent and result commits, and before/after
  receipt publication. A test-first operation-ID collision exposed and fixed a
  gap between intent and final-result reservation. Full results are in the M4
  implementation report; native Unix runtime validation remains open.

## ADR-023 — Scope-bound native drain evidence

- **Date/status:** 22 October 2026; accepted as M4 source-proof groundwork.
- **Problem:** serialized run flags and adapter capability declarations cannot
  prove that a particular managed process tree was independently observed empty.
  The supervisor and store need the same opaque proof type without a crate cycle.
- **Decision:** move the existing safe native process backend and LaunchSpec into
  `baton-platform` behind an opt-in `process` feature. Supervisor still owns each
  live control object. No extra service or execution endpoint is introduced.
  Existing pinned process/job dependencies move with the backend; no dependency
  version is upgraded. Store and supervisor consume the shared native types.
- **Binding:** every actual spawn gets a fresh random scope ID. A native binding
  requires the original in-memory launch permit and an actual process-tree object
  with the matching launch fingerprint. The fresh path rechecks pending transfer,
  owner/epoch/lease and run state before recording Spawned and binding evidence.
  Exact metadata retries return the historical result without minting a permit
  or spawning a process. Session still validates before physical spawn.
- **Drain:** an opaque borrowed proof can be constructed only after the owned
  backend reports empty membership. Windows queries the actual job; Unix process
  groups never mint this proof. The store matches scope ID, launch fingerprint,
  PID and run binding, then atomically records terminal execution, immutable drain
  evidence, task-event hash, audit and idempotent result. Native operations use
  their own scope-bound request domain. Raw observations cannot create these rows.
- **Reading:** serialized scope/drain records are historical facts, never live
  proof objects. Reads verify task-event hashes, actor/epoch/run binding and the
  terminal execution event. Missing execution evidence refuses with CORRUPT_STORE.
  Transfer preflight attaches this verified historical drain record when present
  and rechecks it on exact retries. It does not promote it to a fencing guarantee.
  No PID-based control or proof reconstruction is provided after a restart.
- **Limits:** an empty job proves only managed-tree drain. It does not certify
  adapter coverage, unrestricted filesystem/network fencing, source isolation,
  complete event cursors or settled external effects. PREPARED and activation
  remain unavailable. Scope identity is local; exported records are not remote
  authority. Same-user privileged interference remains outside isolation claims.
- **Compatibility/tests:** schema 7 adds immutable native scope/drain rows and
  refuses schema 1–6 without migration. M4-NP01–04 cover native stop/restart,
  unrelated-scope rejection, synthetic status rejection, missing journal evidence
  and exact metadata retries. The implementation report records actual native
  tests and cross-target type checks separately.

## ADR-024 — Durable receiver staging and ownership CAS behind closed proof gates

- **Date/status:** 22 October 2026; accepted as an M4 storage increment, not a
  production transfer or full milestone exit-gate claim.
- **Decision:** implement PREPARED -> RESTORING -> READY_TO_COMMIT -> COMMITTED
  storage behavior and recovery now, while the engine's missing production
  fencing/isolation, adapter/event and effect checks continue to fail closed.
  Activation explicitly returns EXECUTION_NOT_ENABLED; committed metadata is
  not writer permission and cannot mint an ordinary launch permit.
- **Staging:** the designated receiver reserves a new restore ID and the complete
  stage operation fingerprint before filesystem work. IDs are unique across
  transfers, ordinary operations and forensic restore intent. Completion uses
  the existing immutable receipt and validates actual receiver bytes/index.
- **Commit:** recheck receiver content, source equivalence and source reservation
  expiry. Compare-and-swap the source epoch once, install the receiver/new epoch
  in COMMITTED with a fresh pending lease, and commit transfer/task/audit/retry
  data atomically. Reads cross-check task ownership against the audited committed
  transfer. Expiry/recovery retain receiver ownership; no old-source rollback or
  automatic activation is allowed. Cached results are historical, not capabilities.
- **Validation boundary:** a private task-scoped `cfg(test)` checker supplies
  closed fixture assumptions about source fencing/isolation, event coverage,
  adapter/effects and storage durability. Real CAS, source comparison, receiver
  restore/verification, authentication, SQLite and process kills are exercised.
  The checker is absent from normal libraries/daemon binaries, has no feature or
  runtime switch, and accepts no client flags. Its assumptions do not establish
  production safety, power-loss durability or real vendor compatibility. Tests
  also remove those assumptions after staging and verify commit stays blocked.
- **Source comparison:** recapture into private CAS with the original manifest
  binding/included paths and compare hashes. This does not modify source bytes;
  a changed comparison may leave unreferenced private CAS objects. No agent tool
  or external side effect is replayed. Receiver edits and unknown destinations
  are preserved and refused.
- **Compatibility:** schema 8 adds staging-identity indexes and task states
  COMMITTED/RECOVERY_REQUIRED. Schema 1–7 remains preserved and refused without
  migration. No dependency, frontend, plugin, cloud or vendor integration is added.
- **Tests:** M4-TX01–09 cover the storage tracer, real restoration, role/identity
  reservations, source/receiver drift, evidence loss, concurrent CAS, six actual
  kill boundaries, expiry/recovery and task/grant mismatch. CLI tests use the real
  production checker and verify denied stage/commit, no receiver creation and
  disabled activation. Full results are in the M4 report.

## ADR-025 — Fresh receiver launch intent without serialized process replay

- **Date/status:** 23 October 2026; accepted as an experimental M4 activation
  increment. The production transfer and vendor ACTIVE gates remain open.
- **Decision:** reuse the existing LaunchSpec, opaque LaunchPermit and native
  supervisor. Trusted embedding supplies the launch descriptor/run ID/capabilities
  separately from the serializable ActivateReceiver request. Include them in a
  separate request fingerprint domain, authenticate the receiver, validate the
  committed epoch, restored cwd/content, capabilities, engine boundary checks and
  lease. Persist ACTIVATING, pending run, transfer journal, audit and retry result
  in one SQLite transaction before returning a fresh-only in-memory permit.
  Neither CLI/IPC nor saved request JSON can supply this trusted launch context.
- **Retries/crashes:** an exact retry returns historical run data and no permit.
  Restart cannot recover a lost permit; explicit reconciliation is required.
  Do not turn an uncertain launch into arbitrary shell replay. Competing launch
  requests issue at most one permit. Source and receiver history coexist under
  a unique task/epoch run constraint. Old source authority never returns.
- **Native boundary:** the existing supervisor validates before launch and binds
  the actual native scope with the original permit. Validate lease time again
  after potentially slow receiver checks. Native scope binding rechecks current
  authority but does not require pristine receiver files after the child starts.
  A native spawn is not a validated vendor handshake: task/transfer remain
  ACTIVATING/BLOCKED. Production proofs remain missing and prevent permit issuance;
  positive unit tests explicitly use the existing private boundary assumptions.
- **Integrity/recovery:** activation intent retains immutable run binding,
  launch fingerprint and capabilities, cross-checked against current run metadata
  on task/transfer reads. Process/supervisor uncertainty and expiry preserve the
  committed receiver/epoch; no automatic new run or old-source resume is allowed.
- **Compatibility:** database schema 9 adds ACTIVATING and one run per task/epoch;
  schema 1–8 stores are preserved and refused without migration. Manifest/envelope
  schema remains 0.1.0. No dependency, vendor API, frontend or cloud change.
- **Validation:** M4-AC01–08 cover fresh/retried/competing launches, real native
  scope binding, source history, wrong cwd, file drift, corrupt run metadata,
  validation expiry and lost supervision. Four real kill points cover both sides
  of activation-intent commit and native-scope binding. See M4-IMPLEMENTATION.md
  for actual check results and portability/security limits.

## ADR-026 — Native-scope-bound receiver readiness, with explicit vendor limitation

- **Date/status:** 23 October 2026; accepted as an experimental M4 protocol
  increment. It does not open production transfer or certify a real vendor.
- **Decision:** distinguish native spawn, adapter acknowledgement and continuation
  quality. A normalized schema-0.1.0 ReceiverSession binds an opaque session ID
  and reported adapter metadata to the full committed RunBinding. The trusted
  Session supplies an opaque borrowed proof of its sampled running root. Store
  requires the original native scope/PID/fingerprint, current receiver/epoch/lease,
  pending activation, no earlier recorded tools/gaps/effects, valid artifacts and
  engine boundary checks. Session ID uniqueness is enforced in schema 10.
- **Authority:** generic trusted JSON and local control cannot acknowledge
  readiness. Native-context fingerprints bind the full acknowledgement and scope.
  A valid acknowledgement persists ACTIVE metadata, historical readiness evidence,
  task event, audit and retry result in one transaction. It does not mint a new
  process permit. Recorded data cannot adopt a process or reconstruct control.
- **Quality:** a generic normalized handshake is not a validated Codex/Claude
  session. ACTIVE metadata stays BLOCKED with RECEIVER_ADAPTER_UNVALIDATED.
  Production source/adapter/boundary checks still prevent launch. Positive unit
  tests supply the same explicitly limited closed-boundary assumptions used in
  prior M4 increments; no production flag enables them. These are protocol tests,
  not a product handoff or an assertion that hidden model state transfers.
- **Native limits:** the running proof is an observation at a sampled instant,
  not future liveness, complete containment, event coverage or a vendor attestation.
  On readiness failure Session terminates its owned tree and records loss.
  Expiry, loss and stop preserve the receiver/epoch in recovery. Persist native
  drain evidence before the recovery event; retain the strict terminal-journal
  check rather than weakening it to accept an altered ordering.
- **Retries:** after activation intent, cached staging/commit replies remain
  historical. Continue verifying required checkpoint hashes and grant integrity;
  do not treat subsequent legitimate receiver edits as corruption of the old
  commit. Forensic restoration retries still verify actual receiver content.
  No request replay launches or overwrites a worktree.
- **Compatibility/validation:** schema 10 adds readiness records, unique session
  identities and ACTIVE task metadata; schema 1–9 is preserved/refused without
  migration. M4-HS01–08 use actual managed child acknowledgements, two coordinator
  kills, unrelated-scope/JSON rejection, gaps, expiry/loss and drain ordering.
  The fixture makes only known local writes after ownership commit. No dependency
  or upstream vendor interface changes. Actual results are in the M4 report.

## ADR-027 — Successive sources, receipt-bound capture and fresh tool admission

- **Date/status:** 23 October 2026; accepted experimental M4 implementation.
  Production transfer guarantees and real-vendor validation remain unavailable.
- **Decision:** distinguish a current committed writer grant from the pending
  transfer slot. Permit ACTIVE receivers to request the next transfer, entering
  QUIESCING without allocating a new owner. Require one committed grant per epoch.
  Abort, expiry or uncertainty retains the current owner/epoch in recovery (or
  SUSPECT before any grant); it never silently resumes a source. Known live scopes,
  event gaps and unresolved effects reduce safety facts even in fixture tests.
- **Workspace:** derive the current source from its audited grant's receiver
  stage and enrolled common Git identity. Keep ordinary vault-overlapping capture
  forbidden. A dedicated read-only API accepts only a completed receipt-bound
  receiver with matching task/repository and exact next epoch. Receipt format 2
  pins the canonical Git administrative directory. Legitimate receiver edits may
  become the next source; direct forensic restore retries still verify bytes.
  Unrecognized targets, legacy receipts and changed identity are preserved/refused.
- **Admission versus facts:** add ToolReserved and fresh-only opaque ToolPermit.
  Native host checks the original scope, canonical launch_worktree, current
  actor/task/run/epoch/lease, normalized sequence, no gap/effect and no pending
  transfer. Consume the permit by value with fresh authority checks. Cached or
  restarted requests return history without permission. Only trusted classified
  ReadOnly/LocalReversible operations in task scope `.` are admitted; other
  scopes/classes are disabled until an operation policy/reconciler exists.
  No command, arbitrary shell or persisted replay recipe is introduced.
- **Limits:** classification is trusted host policy, not evidence that arbitrary
  shell commands are safe. The host must constrain concrete operations. This is
  no filesystem/network sandbox or global fence. Late observed starts during
  QUIESCING are retained and force recovery. Unfinished reservations become
  UNCERTAIN on loss; they are never replayed automatically. Production checks
  remain closed and only private unit fixtures supply missing assumptions.
- **Compatibility:** state schema 11 adds these semantics and launch cwd binding;
  schemas 1–10 are refused without modification. Envelope/manifest schema stays
  0.1.0; restore receipt format is separately versioned at 2. No auto-migration,
  dependency, frontend, MCP, plugin or upstream vendor interface change.
- **Validation:** M4-SEQ01–05, M4-TL01/02 and M4-SEQ-CAS/SEQ-CAS02 cover successive
  native fixture handoff, retained receiver edits, concurrent requests, stale
  epochs, admission revocation, retained late facts, four actual first-tool
  coordinator kills and receipt identity/compatibility failures. Exact results,
  scope of the Windows simulated gate and outstanding OS/release limits are in
  M4-IMPLEMENTATION.md. Real Codex source work belongs to M5, not this fixture gate.

## ADR-028 — Managed App Server stdio and explicit launch environments

- **Date/status:** 25 October 2026; accepted experimental M5 handshake increment.
- **Decision:** keep the existing audited Session/native process scope. Extend
  LaunchSpec with explicit piped stdin/stdout and a bounded explicit environment;
  keep stderr null and inherited environment cleared. Include transport and
  canonical environment in the existing launch fingerprint. Preserve the old
  fingerprint for null transport with no environment. Take native pipe ownership
  once and use existing Tokio for bounded asynchronous I/O; no new daemon,
  shell replay, SDK language runtime or public process endpoint is added.
- **Reason:** the previous null-only launcher cannot carry App Server JSONL.
  A transport change must not reuse another launch's permit. A private temporary
  CODEX_HOME permits native interface testing without using user configuration
  or credentials. Explicit environments are trusted host input, not agent JSON;
  do not persist their contents as launch recipes or claim secret encryption.
- **Protocol:** initialize/initialized only in this increment, stable API mode,
  explicit gateway OAuth, exact reply correlation, strict shape and 64 KiB frame
  bound. Deadline is positive and at most 30 seconds. Vendor home/user-agent/error
  payloads do not enter diagnostics or the exposed runtime report. Unexpected
  protocol messages fail closed; later event support needs its own normalization.
- **Lifecycle:** failed/cancelled connect terminates its owned scope and records
  loss where storage permits. Cancellation Drop cleanup cannot guarantee an audit
  commit if storage is unavailable. After successful connect, callers must stop
  explicitly to journal the boundary; ordinary Source drop only provides native
  teardown. Do not turn saved binding/PID/runtime fields into restart authority.
  Windows Job Object and Unix process-group limitations remain unchanged.
- **Compatibility:** library/envelope version remains 0.1.0; database remains
  schema 11 because no stored record layout changes. New adapter diagnostic codes
  fail decoding on older builds rather than silently downgrading. Installed Codex
  advanced from 0.163.0 to 0.164.0; public schemas were regenerated and the actual
  0.164.0 local handshake tested. This does not establish supported real source
  sessions, event completeness, sandboxing, source fencing or M5's exit gate.
- **Validation:** M5-C02/C03/C04 use bounded protocol cases and actual native
  children, including timeout/cancellation and exclusive-handle teardown. Explicit
  M5-C05 initializes installed Codex in a new private home without a model turn.
  Full check results and remaining constraints are in M5-IMPLEMENTATION.md.

## ADR-029 — Durable adapter correlation without claiming source safety

- **Date/status:** 25 October 2026; accepted experimental M5 lifecycle increment.
- **Decision:** extend the existing execution journal with bounded adapter reports
  and explicit gap markers. Reuse authenticated run/actor/epoch/lease and SQLite
  transactions; bind each adapter event by hash to its task event. Validate the
  evidence on reads and exact cached acknowledgements. Derive stream/thread/turn/
  item correlation from immutable journal entries, preserving task/run scope.
- **Retries:** run-scoped BLAKE3-derived opaque IDs identify normalized event phases
  and effects. Duplicate normalized observations reuse the original sequence and
  cached operation; changed facts under the same key conflict. They are metadata
  acknowledgements, not command replay or current authority. The normalized subset
  intentionally omits raw command/output, so dedup is not a raw-message equality
  or cryptographic vendor-attestation claim.
- **Uncertainty:** missing predecessors are sticky gaps. Orphan completions retain
  an uncertain effect without inserting a fictitious observed start. Current
  command/file-change effects are Unknown even after reported success. Loss of
  supervision marks the unconfirmed event tail as a gap, including when an earlier
  session failure has already stopped capture. Turn completion cannot mark a tree
  empty. New turn/tool activity during QUIESCING forces source recovery as before.
- **Coverage:** capture only the four supported lifecycle methods and supported
  command/file-change shapes from generated Codex 0.164.0 schemas. Unknown items,
  unsolicited requests, malformed data and wrong bindings fail closed with
  gap/loss. No real model turn, automatic approval, source activation, full event
  coverage or safe boundary is implemented by this increment. Raw outputs,
  reasoning and agent text are omitted; they must not be invented as evidence.
- **Compatibility:** schema 12 stores the new observation variants; schemas 1–11
  remain preserved/refused without migration. Envelope schema remains 0.1.0. The
  adapter reuses the existing BLAKE3 dependency. No dependency version upgrade,
  frontend, MCP, plugin, cloud or new process-execution endpoint is added.
- **Limits/validation:** initial correlation and dedup read the bounded existing
  journal; event-processing performance is unbenchmarked. M5-E01–06 cover reported
  exit facts, provenance/omissions, orphan/foreign/conflicting events, durable
  retries after reopen, evidence corruption, cancellation and timeout with actual
  native fixture peers. These peers report synthetic events; they do not run
  Codex tools. Real event capture and continuation require later M5/M6 evidence.

## ADR-030 — Cancel-only approvals with explicit host provenance

- **Date/status:** 25 October 2026; accepted experimental M5 increment.
- **Decision:** extend the existing journal, not execution permissions. Recognize
  command/file-change approval requests and reply only cancel. Commit cancellation
  intent before pipe I/O and written evidence after write/flush. Both records are
  HostReported; serverRequest/resolved is separately AdapterObserved. Resolve its
  request ID through the durable intent because the vendor notification omits turn
  ID. No accept, session grant, policy amendment or model-turn API is enabled.
- **Identity/retries:** retain signed integer versus string RPC identity. String
  IDs use the existing bounded adapter-ID subset. Phase keys bind the full run,
  adapter and thread; stored facts also bind turn/item/tool. Changed facts conflict.
  Reuse after resolution is unsupported and refuses before another reply, rather
  than guessing whether a new operation or retransmission was intended. Historical
  cached journal results never reconstruct a native pipe or permission.
- **Coverage/privacy:** retain only start/completion IDs and type for userMessage,
  agentMessage, reasoning, plan and contextCompaction. Omit their contents entirely;
  do not infer hidden model state or create tool effects from passive metadata.
  Unsupported requests, malformed scopes or offered decisions without cancel stop
  the owned scope with gap/loss. Missing starts remain sticky gaps.
- **Safety:** host-written bytes do not prove vendor receipt, and vendor resolution
  does not prove effect absence. Tool effects remain Unknown/UNCERTAIN, including
  declined items. Failed storage can prevent final loss journaling; retain only
  committed phases and native teardown, never adopt a process or replay work.
  Transport deadlines bound I/O waits, not synchronous storage/cleanup. Cancellation
  during QUIESCING cannot issue writer authority or renew an epoch.
- **Compatibility:** schema 13 stores the new kinds/origin; schemas 1–12 are
  preserved/refused without migration. Envelope schema remains 0.1.0. Generated
  Codex 0.164.0 schemas define cancel and typed RequestId. The overview additionally
  describes availableDecisions; if present it must include cancel. No dependency
  version change, cloud upload, UI, plugin or MCP package is introduced.
- **Validation/limits:** M5-A01–07 and M5-P01/P02 exercise pure protocol/parser and
  actual managed synthetic peers, including two storage-failure boundaries. These
  are not real Codex tool execution or source safety proof. Initial source writer
  admission, remaining event coverage, interruption and real M5 source work remain
  open. See M5-IMPLEMENTATION.md for commands, results and remaining gates.

## ADR-031 — Audited turn interruption without replay or drain inference

- **Date/status:** 25 October 2026; accepted experimental M5 increment.
- **Decision:** keep the existing initialized Source, Session and execution journal.
  For an observed active turn, journal TurnInterruptPlanned as HostReported intent
  before sending turn/interrupt, then TurnInterruptWritten after write/flush. Intent
  atomically moves the run to STOPPING. Capture the correlated empty success reply
  as AdapterObserved TurnInterruptAcknowledged. Unsupported/rejected/malformed or
  uncorrelated replies fail closed; raw rejection content is discarded.
- **Binding/retries:** deterministic opaque request ID binds run/actor/epoch/lease,
  adapter/version, thread and turn. Phase-specific control operation keys preserve
  durable idempotency. An exact live retry with written evidence returns current
  metadata without another send. Intent without written evidence is ambiguous and
  cannot be replayed. Saved records never reconstruct a transport or native permit.
- **Safety:** request, written bytes, reply, terminal turn and native drain are
  separate facts. The reply may arrive after turn completion; correlation uses
  durable intent rather than the current active turn. Neither reply nor terminal
  status marks the tree stopped, renews ownership or reconciles Unknown effects.
  Failure/cancellation uses the existing owned-scope teardown and gap/loss guard;
  unavailable storage may prevent its final audit. I/O deadlines remain separate
  from synchronous storage and cleanup time. Real vendor source safety is unproven.
- **Compatibility:** database schema 14 adds interrupt journal variants; schemas
  1–13 remain preserved/refused without migration. Envelope schema stays 0.1.0.
  Public Codex 0.164.0 interrupt schemas and official documentation were checked.
  The parser accepts an empty success object, rejects mixed result/error even if
  null, and retains no error payload. No positive approvals, model turns, cloud,
  frontend, MCP/plugin or dependency version changes are introduced.
- **Validation/limits:** M5-I01–06 test pure protocol/parser and actual managed
  synthetic peers with interleaving, duplicate live requests, wrong scope/ID,
  rejection, timeout and two storage failure phases. These peers do not execute
  Codex tools. Source writer admission, remaining real event coverage, real source
  work and safe stopped boundary still gate M5. See M5-IMPLEMENTATION.md.

## ADR-032 — Validate source tool control through a loopback vendor tracer

- **Date/status:** 25 October 2026; accepted experimental M5 investigation.
- **Tension:** an ownership flag or adapter self-report cannot establish that real
  Codex writes pass through Baton. Existing native ToolPermit is available for
  bounded initial-task operations; the adapter callback/control path is missing.
- **Decision:** add a test-only opt-in installed-vendor tracer using an ephemeral
  loopback Responses fixture, synthetic repository and fresh explicit home. Disable
  shell/apps/agents/search and inspect the actual advertised tool set. Generate
  experimental public schemas for dynamic tools, leaving production initialization
  and execution grants unchanged. Record a stream gap before native teardown.
- **Evidence:** Codex 0.164.0 advertised image/goals/input tools despite shell
  suppression. Canonical goals/view_image feature flags reduced the observed set
  to request_user_input plus the registered baton_probe function. The strict
  Baton-only diagnostic still fails. This is not proof that the residual question
  tool writes files, nor evidence that all unadvertised paths are blocked.
- **Resolution:** keep source writer admission disabled. Implement/validate explicit
  client-mediated input and dynamic-tool callbacks, then bind concrete bounded
  actions to the existing native permit. Do not fabricate capability proof or
  change the transfer gate to make the experiment pass. No new ACTIVE source flag
  or generic shell bridge is introduced by this investigation.
- **Privacy/limits:** no account credentials or real model are used. Model requests
  are received on the fixture's loopback listener; no kernel-wide network blockade
  is claimed. Raw requests, instructions and sessions remain temporary. The HTTP
  reader refuses auth/encoding/alternate routes and is bounded; only tool names
  are retained in diagnostics. Native containment limitations remain unchanged.
- **Validation:** M5-G00 tests the probe reader; G01's observed failure is explicit
  compatibility evidence, not a passed acceptance case. Schema stays 14. Exact
  commands/results and next M5 work are in M5-IMPLEMENTATION.md.

## ADR-033 — Refuse callbacks without fabricating answers or tool effects

- **Date/status:** 25 October 2026; accepted experimental M5 increment.
- **Decision:** recognize generated 0.164.0 item/tool/requestUserInput and
  item/tool/call, using existing native Source transport, journal and teardown.
  User input has no schema-defined cancel answer: record a HostReported refusal
  plan and stop with EXECUTION_NOT_ENABLED, never answer for the user. Dynamic
  requests receive only a fixed success:false result after durable refusal intent;
  record Written after flush. Resolved remains a separate vendor observation.
- **Binding:** callback metadata carries typed RPC ID, opaque itemId/callId
  correlation, kind and BLAKE3 raw-frame fingerprint. Share server-request phase
  keys with approvals to prevent cross-method ID reuse. Payload/scope changes
  conflict, terminal IDs cannot resend, and a Written phase cannot follow resolution.
  Byte-level fingerprints intentionally reject changed serialization, not merely
  changed semantics; no compatibility fallback silently ignores such a conflict.
- **Privacy/provenance:** question/argument text is transient and omitted from the
  journal. Fingerprints remain private metadata, not encryption or an attestation.
  Dynamic lifecycle items are ClientToolCall metadata; a request arriving at this
  host is not proof that a host action executed. No tool effect or positive grant
  is created by a negative result. Existing Unknown effects remain unresolved.
- **Compatibility:** schema 15 adds callback kinds and phases; schemas 1–14 are
  preserved/refused without migration. Envelope schema stays 0.1.0. Stable Source
  initialization is unchanged; no dynamic tool registration/model-turn API is
  enabled. Native callback probes use explicit experimental initialization only
  inside tests, a new home, synthetic repository and loopback mock provider.
- **Validation/limits:** M5-D01–06/U01–04 cover protocol/parser, actual managed
  synthetic peers, raw payload conflicts, cross-method reuse, two storage failures,
  no invented answers and native vendor compatibility. Native raw-protocol probes
  show failure forwarding and stop-on-input, not complete production Source capture
  or actual model work. The strict Baton-only inventory experiment is still a
  separate known failure. Bounded positive tool admission, real source work and
  independent source boundary proof remain open. See M5-IMPLEMENTATION.md.
