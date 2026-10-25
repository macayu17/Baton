# M3 implementation plan and evidence

Status: experimental process-control/effect-ledger slice implemented, disabled by
default. Full M3 exit gate remains open; no automatic transfer is enabled.

This is the historical M3 schema-3 report. M5 now uses schema 15 and adds the
experimental local CLI/daemon and OS identity boundary; see `M4-IMPLEMENTATION.md`.

The complete-project goal authorizes continued roadmap implementation. Earlier
native OS, security and durability release gates remain open.

1. Version run bindings, adapter capabilities, normalized event cursors and effect
   classifications (FR-03/14/15, INV-04/09). Test strict decoding, gaps, ambiguous
   completion and explicit reconciliation before implementing transitions.
2. Persist launch intent before execution; issue a nonserializable one-use permit
   only for the fresh committed request. Journal observations and reject stale
   epochs, duplicates with changed input, and unsafe reservation cancellation
   (FR-04/12, INV-06/07). Restart never replays a launch or trusts a recycled PID.
3. Establish Windows kill-on-close Job Objects before resuming the suspended
   child; query job membership independently of root exit. Unix process groups
   report cooperative limits. Tracer: a real fake-agent child survives its parent
   and remains active until the managed tree is stopped (FR-03/06, T-006).
4. Exercise lost external acknowledgement, event gaps, process/owner crashes,
   missing capabilities, bounded stop, idempotency and source lease expiry
   (T-006/T-007/T-009, INV-04/06/09). Run default/all-feature workspace checks.

No managed process may infer permission to write from this experimental library.
Trusted embedding code supplies explicit executables/arguments, with no generic
shell replay or IPC command endpoint. OS process containment does not sandbox
filesystem/network access or fence external services. Full transfer/activation
remains M4, and real vendor capability evidence remains M5/M6.

## Architecture and implemented behavior

`baton-protocol` defines strict schema-0.1.0 run bindings, observations,
capabilities, normalized events and effects. `baton-store` commits these records
and their operation/audit/task-event entries atomically. `baton-supervisor` owns
the live process handle and OS controls; serialized records and PIDs cannot
recreate that control. There is no daemon, CLI, network listener or arbitrary shell
endpoint in this milestone.

The launch transaction creates LAUNCH_PENDING. Only a fresh successful request
returns a noncloneable, nonserializable LaunchPermit. Exact retries and reopened
stores return the old record without another permit. Immediately before launch,
the supervisor checks actor/lease/epoch/expiry and the complete pending record,
then matches a length-delimited BLAKE3 fingerprint of executable/cwd/arguments.
Inputs require absolute existing paths and bounded arguments. The executable
itself is trusted embedding input, not a sandboxed untrusted program or a binary
attestation. Environment is cleared, stdio discarded, and Windows launches hide
the console. No credentials or raw command payloads are persisted or logged.

Windows starts suspended through process-wrap. A post-spawn hook assigns the
child to a separately owned unnamed, non-inherited kill-on-close Job Object
before the wrapper resumes execution. The wrapper's nested job supplies explicit
termination; the owned job independently supplies membership queries and
supervisor-crash teardown. No breakaway flags are enabled. Query/control errors
fail closed; the safe query wrapper has a roughly 1,024-process bound. Root exit
and interrupt acknowledgement are explicitly separate from an empty job.

Unix creates a process group and can terminate it while the leader identity is
retained. It never signals a group after reaping its leader, avoiding recycled
identifier targeting. A group cannot prevent setsid or escaped descendants and
does not survive supervisor crash as an enforceable containment guarantee. Root
exit therefore reports Unknown and RECOVERY_REQUIRED, never confirmed tree drain.
This limitation is deliberate and remains an implementation task, not an exit
gate waiver.

Run states are LAUNCH_PENDING, RUNNING, STOPPING, STOPPED and RECOVERY_REQUIRED.
Each accepted observation receives a monotonic run sequence, coordinator time,
task-event sequence, operation ID, actor/epoch binding and explicit origin.
Evidence hashes are explicitly unavailable for discarded raw output; a tool exit
is not a verified test result. Source sequence gaps remain sticky. Session accepts
adapter tool/interrupt observations but rejects fabricated native process facts.
Store observations remain trusted low-level embedding operations and must not
be forwarded directly from future IPC/MCP clients.

Every started effect has an explicit class. Read-only and reversible local
completed actions can be recorded complete; interrupted, unknown, uncertain local
and external actions remain uncertain. Stopping a tree or losing supervision
marks incomplete operations uncertain. External tool success alone cannot prove
resource state. Hash-only reconciliation returns BLOCKED; no validated resource
reconciler exists yet. There is no replay method.

Capability booleans record declarations. Every M3 boundary assessment includes
ADAPTER_UNVALIDATED until installed-runtime/event-coverage validation is added.
Other reasons identify unconfirmed process drain, unresolved effects, gaps,
missing capabilities and missing mutation gating. These records cannot activate
a writer or turn an M2 forensic checkpoint into VERIFIED continuity.

Native stop still terminates owned processes if the lease has expired. If the
stopped observation is no longer authorized, the record becomes
RECOVERY_REQUIRED, while the task remains SUSPECT with its old epoch/owner. No
takeover or resurrection occurs. Launch/control errors expose safe
SUPERVISION_FAILURE diagnostics; bounded drain exposes DRAIN_TIMEOUT. Lost
launch/control identity is not recovered by polling arbitrary PIDs.

New stores use database schema 3. Schema 1/2 refuse without rewrite/migration.
This increment permits one run reservation per task. Existing metadata cancel
is blocked once a run exists; M4 must supply safe retirement/ownership lifecycle.
Execution events are append-only, with persisted counts and contiguous sequence
checks detecting missing middle/tail records. No user database was migrated.
ADR-019 records the choices and their rollback and authority limits.

## Files

Created `crates/baton-supervisor/Cargo.toml`, `src/lib.rs`, `src/engine.rs`,
`src/engine/native.rs`, `tests/supervision.rs` and `tests/support/fake_agent.rs`;
`crates/baton-protocol/src/execution.rs` and `tests/execution.rs`;
`crates/baton-store/src/engine/execution.rs`; and this report.

Updated workspace manifests/lockfile, protocol metadata/diagnostics/exports and
canonical-code tests, store transaction dispatch/schema/unit and integration
tests, CI default-feature Clippy, README, roadmap, architecture, protocol,
security, references, testing, historical M1/M2 reports and ADR-019.

## Verification evidence

Host: Windows NT 10.0.26300.0; rustc/Cargo 1.98.0 Windows GNU; Git
2.51.0.windows.1. All processes were synthetic Rust fixtures, with writes confined
to temporary fixture directories. Codex and Claude were not invoked.

| Requirement / invariant | Evidence |
|---|---|
| FR-03/06, INV-06/09 | T-006 native test: root exits, owned job still lists live writer, explicit tree stop drains it |
| FR-06/07/12, INV-06 | Actual live process remains after lease expiry; task becomes SUSPECT, claim refused, old epoch retained |
| FR-12, INV-06 | Hard-killed supervisor closes job; held descendant file handle becomes exclusively openable |
| FR-14, INV-04 | Fake deployment commits once, loses acknowledgement, effect remains UNCERTAIN after stop; no replay |
| FR-03/15, INV-09 | Interrupt acknowledgement remains STOPPING; adapter cannot submit TreeStopped through Session; capability claims do not certify coverage |
| FR-04/12, INV-07 | Hard kills before/after intent commit; permit issued only for fresh committed request, never cached/restarted launch |
| FR-03/04/12, INV-09 | Durable source gaps/effects survive reopen; deleted execution tail blocks reads; stale epoch and cross-task actor refused |
| FR-12/15/19 | Changed launch fingerprint refused; invalid executable records RECOVERY_REQUIRED; expired permit and old DB schema refused |

Verification commands use `cargo +stable-x86_64-pc-windows-gnu`:

- `fmt --all -- --check`.
- `clippy --workspace --all-targets --all-features --locked -- -D warnings`.
- `build --workspace --all-targets --all-features --locked`.
- `test --workspace --all-features --locked`: **64 passed, zero failed**, three
  intentionally ignored subprocess entry helpers invoked by the fault tests.
- Default-feature Clippy and all-target build passed; `test --workspace --locked`:
  **15 passed, zero failed**.

The ignored CAS/store/supervisor entries are subprocess helpers explicitly
invoked by parent fault tests, not skipped acceptance tests. Crash timing uses
observed markers. Clippy's zombie-process lint is locally allowed only in the
fake-agent binary that deliberately exits before its child. No performance or
power-loss result is inferred from these tests.

## Remaining gates and next work

Native macOS/Linux/Windows MSVC validation is pending; configured CI is not an
observed run. Unix strong containment and supervisor-crash cleanup are incomplete.
Windows job containment does not cover service/WMI/remote actions or filesystem
and network sandboxing. OS-bound IPC authentication, durable credential recovery,
Windows ACL/directory durability and complete vendor event evidence remain open.
Raw-output capture and resource reconciliation remain disabled. This is not the
complete M3 crash matrix or a production VERIFIED safe-boundary implementation.

The next roadmap milestone is **M4: transfer state machine and local CLI**
(FR-08/09/12/13, INV-02/07/08; T-003/T-008/T-009). Continue implementation toward
the complete-project goal, preserving the earlier gates. M4 must enforce native
proof and authenticated identity and must not trust JSON process/capability
claims as permission to commit or activate a writer.
