# Baton — Verification, Fault Injection, and Acceptance Test Plan

**Document version:** 1.0 | **Scope:** R0 local continuity, R1 installable integrations, R2 distributed teams

## 1. Why tests are part of the product contract

Baton transfers mutable developer work between independent AI-agent runtimes. A convincing demo is **not** proof that ownership, snapshots, or side effects are safe. Every release requires three separate forms of evidence:

1. **Deterministic correctness:** state-machine properties, durable artifacts, idempotent operations, epoch fencing.
2. **Integration coverage:** real Codex and Claude versions using documented adapter surfaces on supported OSes.
3. **Product continuity quality:** receiving agent solves interrupted tasks better than the Git-only and manual-handoff baselines.

A failed-closed transfer (honest `BLOCKED`) is a **correct outcome** in a test involving uncertain mutations. Do not count an unsafe automatic resume as success.

## 2. Traceability convention

Every automated test should cite affected `FR-XX` (from `PRD.md`) and `INV-XX` (from `docs/PROTOCOL.md`). Report OS, Git version, Rust version, Codex/Claude version, working-tree fixture and whether the test used mocked or live agents. Preserve anonymized logs and hashes of inputs/outputs without including secrets.

Release artifacts should include a coverage report with: requirement ID, specific test, tested revision, environment, outcome, reproducibility, known limits, and reviewer.

## 3. Test layers

M5-C01 is installed-version/public-schema discovery, not real-work acceptance.
M5-C02 tests initialize correlation, strict reply shapes, duplicate fields, size
bounds, truncated input, EOF, vendor rejection and native timeout. M5-C03 tests
actual managed stdio, one-time pipe ownership and fingerprinted explicit
environments, including duplicate-name refusal. M5-C04 waits for an actual peer's
received-initialize marker, cancels connect and checks recovery plus Windows
exclusive-handle release. These trace to FR-03/12/15 and INV-04/06/07/09 as bounded
adapter/lifecycle subcases. M5-C05 is an explicit opt-in installed Codex handshake
using a new private home and no model turn. It is ignored in the ordinary suite
and must be reported separately from synthetic tests. The platform pipe_child
ignored entry is a helper explicitly invoked by C03. See M5-IMPLEMENTATION.md.

M0 executable test identifiers and their bounded FR/INV associations are recorded
in [M0-BASELINE.md](M0-BASELINE.md). These harness/contract tests do not substitute
for T-001 through T-010 or establish transfer invariants.

M1's implemented metadata subcases, real process-kill boundaries, and unresolved
parts of T-001/T-002/T-008 are listed in `M1-IMPLEMENTATION.md`. Run them explicitly
with `cargo test --workspace --all-features --locked`; the store is disabled by
default. The ignored `crash_child` entry is a test-only subprocess helper that
M1-004 invokes explicitly at both boundaries, not a skipped acceptance case.

M2 adds T-004/T-005 artifact subcases, strict manifest tests and native Git
fixture cases in `M2-IMPLEMENTATION.md`. The `cas_child` and `crash_child` ignored
entries are subprocess helpers invoked by their parent tests. CAS boundaries,
SQLite commit boundaries, changed-source synchronization and interrupted receiver
creation use observed markers rather than guessed kill timing. Windows process
kill is tested here; this does not prove Job Object containment or power-loss
durability. Full R0 invariants still require M3/M4 and the remaining matrix.

M3 adds real fake-agent process tests plus durable run/effect tests. The ignored
`crash_owner` helper is explicitly launched and hard-killed by its Windows parent
test; a held child file handle confirms that job teardown terminates the writer.
Separate tests query live job membership after the root exits, expire a lease
while a process lives, lose a fake deployment acknowledgement, corrupt an
execution journal tail, and kill before/after launch-intent commit. See
`M3-IMPLEMENTATION.md`. Unix tests must retain Unknown/RECOVERY_REQUIRED outcomes
for process-group limits; they must not substitute root exit for containment.

M4's current local-entrypoint tests use real native `baton` processes and IPC.

M4 transfer coverage is incremental: four pure policy-model tests exercise
receiver-before-commit denial, monotonic synthetic commit/recovery, mandatory
blockers and degraded acknowledgement. M4-TR01–05 cover persisted intent/retry,
ACL/epoch/expiry, concurrent requests, pending permit validation, hard kills
before/after request commit, damaged records and abort without lease revival.
M4-CLI05 launches actual binaries and hard-kills the coordinator before retry,
then checks preflight, read and abort. These trace to FR-08/09/12/13 and
INV-02/07/08/09 as subcases of T-003/T-008/T-009. They do not establish receiver
staging, actual epoch commit, activation or complete end-to-end transfer.
They cover OS identity recovery, private directories/ACLs, lock exclusion,
strict/bounded envelopes, denied supervisor observations and their audit,
concurrent duplicate claims, daemon hard kill/restart, and forensic CLI
capture/restore with edited-receiver refusal. These do not establish two-phase
transfer or activation; the full M4 gate remains open. See `M4-IMPLEMENTATION.md`.

| Layer | Runs when | Purpose |
|---|---|---|
| Unit/property tests | Every change | Check state transitions, token/epoch rules, manifests, classification, serialization |
| Storage/FS integration | Every pull request | Prove snapshots, staging, restoration, Git worktree isolation |
| IPC/daemon integration | Every pull request | Verify identity, permissions, retries, daemon restart and API behavior |
| Fake-agent adversarial | Every pull request | Exercise controlled starts, exits, child-process leakage, hooks missing, dropped events |
| Real Codex → Claude | Nightly/RC | Prove end-to-end capture/restore/continuation on pinned vendor builds |
| Portability | CI + release candidate | Windows, macOS, Linux path, IPC, process-tree and permissions behavior |
| Cross-machine fault simulation | R2 gate | Partitions, clock skew, interrupted uploads, coordinator outage, stale epoch fencing |
| Benchmark/evaluation | Milestone and release | Measure runtime performance and actual context-continuity value |

## 4. Core property tests (release-blocking)

M5-D01–06/U01–04 cover schema-15 negative callbacks (FR-03/06/12/14/15,
INV-04/07/08/09): protocol phase/provenance/payload binding and cross-method ID
conflicts; scoped parser and omission of raw text; managed native peers receiving
failure-only responses; input-required stop without fabricated answers; changed
payload/resolved-ID reuse; two storage-failure phases. D06/U04 are separately
opt-in actual Codex loopback-provider compatibility probes: failure result is
forwarded to the fixture, or an input request is observed before native stop.
They use raw test protocol, not a complete production Source session or real
model. The strict G01 inventory experiment remains a separate known failure.
Full names/results are in M5-IMPLEMENTATION.md and ADR-033.

M5-G00 tests the bounded credential-refusing HTTP reader used only by the local
vendor tracer. M5-G01 is a separately opt-in installed-Codex inventory experiment,
ignored in the normal suite. It drives a native vendor turn against a loopback
synthetic Responses fixture, not a real model/provider. The strict Baton-only
inventory assertion currently fails with residual request_user_input. Report that
failure separately from ordinary passing tests; it is not an acceptance pass or
source writer grant. Raw probe observations are marked as a gap before native
stop, and no production execution path is enabled. FR-03/06/15/16,
INV-04/06/09; see ADR-032 and M5-IMPLEMENTATION.md.

M5-I01–06 cover schema-14 bounded interruption (FR-03/06/12/15,
INV-04/06/07/09): active-turn/predecessor/provenance rules; native reply-first and
terminal-first interleaving with exact live retry suppression; strict empty reply
and mixed-envelope rejection; unknown ID, vendor rejection and timeout; storage
failure before/after intent commit; wrong turn refused before send. Native peers
are synthetic. Tests retain Unknown effects as uncertain after stop and do not
substitute a reply or terminal turn for native drain. M5-IMPLEMENTATION.md records
the exact names/commands; no real model interruption is certified by these tests.

M5-A01–07 and M5-P01/P02 cover schema-13 cancellation/passive metadata:
protocol provenance and predecessor rules; strict typed IDs and cancel-only wire
responses; native command/file requests with numeric/string IDs; storage failures
before/after intent commit; unsupported methods or missing cancel support;
cancellation during QUIESCING without epoch/writer revival; and resolved-ID reuse
without another reply. Passive fixtures cover all five supported kinds and omit
text/reasoning without creating effects. These trace FR-03/06/12/14/15 and
INV-04/07/08/09. Native peers are synthetic, tool outcomes are reports, cancellation
does not settle effects, and no real model turn is tested. See ADR-030 and the
M5 implementation report for exact test names/results.

M5-E01–06 are bounded lifecycle subcases, not the full M5 exit gate. Protocol tests
retain unknown effects and orphan completions with gaps; turn correlation refuses
rebinds and duplicates. Wire tests preserve reported nonzero exit facts without
private command/output strings and reject unsupported shapes/bindings. Native
tests drive synthetic peer notifications through the owned pipe, deduplicate a
repeat, retain uncertainty after reported success/failure, reopen and retry
without another event, reject changed facts and foreign threads, verify damaged
task-evidence hashes block reads/cached acknowledgements, and mark cancellation/
timeout as gap plus recovery. These trace FR-03/04/10/12/14/15 and
INV-04/07/08/09. Synthetic reported statuses are not independently measured tool
outcomes or real Codex event coverage. See M5-IMPLEMENTATION.md and ADR-029.

M5-E06 verifies that a new vendor turn observed after QUIESCING is retained as a
fact while the source becomes SUSPECT/recovery, epoch stays fixed and its owned
child stops. This closes the new observation channel's source-freeze subcase.

M4-SEQ01–05 exercise A -> B -> A with real native fixture children, local Git/CAS
and SQLite: capture receiver edits at the next epoch, preserve original/earlier
worktrees, reject stale epochs, serialize competing next transfers, and deny
handoff for live scopes, known event gaps or unresolved effects. QUIESCING expiry,
loss and abort retain the current receiver/epoch without renewed writer authority
(FR-08/09/12/13/14, INV-01/02/04/05/06/07/08/09; T-003/T-008/T-009 subcases).
SEQ05 performs four actual coordinator kills before reservation intent, after
intent, after the first known local write and after completion. Restart retains
the corresponding missing/UNCERTAIN/COMPLETED facts; no serialized request mints
a permit or repeats the operation. An exclusive handle and job teardown provide
the fixture's loss evidence, not PID adoption. Positive transfer cases still
use private test-only boundary assumptions; no production safety gate is opened.

M4-TL01/02 distinguish fresh native-host tool admission from actual observed
activity (FR-03/08/12/14, INV-02/04/07/08/09). Raw reservations, external classes,
retries and an unused permit after QUIESCING are denied. A late actual ToolStarted
is retained and forces recovery. The typed child waits for admission before its
known local write; no arbitrary shell replay or vendor session is involved.
M4-SEQ-CAS/SEQ-CAS02 verify the narrow receipt-bound source capture: task, exact
next epoch, request fingerprint, legacy receipt and changed Git identity failures
preserve files (FR-05/09, INV-03/05). The ordinary overlap prohibition stays tested.

M4-HS01–08 cover native-scope-bound receiver acknowledgement, actual fixture-child
response, ACTIVE metadata, historical commit retries after legitimate receiver
edits, rejected JSON/status/PID and unrelated-scope inputs, missing proofs, gaps,
task/grant corruption, source-session denial, expiry/loss and valid native drain
ordering (FR-08/12/13/15, INV-01/02/06/07/08/09). Two actual coordinator kills
bracket readiness commit. Restart preserves owner/epoch and never reconstructs
native control or launch permission. `receiver_fixture_child` is an ignored
subprocess entry explicitly invoked by these tests. Positive cases use the same
private boundary assumptions as M4-TX/AC tests; ACTIVE remains BLOCKED with
RECEIVER_ADAPTER_UNVALIDATED, not a real-vendor or production safety claim.

M4-AC01–08 cover fresh-only receiver launch permits, retained source/receiver run
history, actual native scope binding, duplicate/restarted requests, missing
production checks, precommit/wrong-cwd denial, receiver drift, idempotency-domain
conflicts, corrupt run metadata, expiry during validation, and concurrent launch
requests (FR-08/12/13, INV-01/02/03/05/06/07/08). Actual hard kills occur before
and after activation intent commit and native scope binding. They retain the new
owner/epoch and never reissue a committed permit. Positive cases rely on the same
private closed-boundary assumptions as M4-TX tests; the spawned process is the
test binary with --help. This is not vendor ACTIVE or production fencing proof.

M4-TX01–09 exercise durable staging, reserved-operation isolation, source drift,
receiver edits, concurrent commits, loss of boundary assumptions, committed expiry,
recovery and task/grant corruption (FR-08/09/13, INV-01/02/03/05/06/07/08).
Actual child processes are killed before/after staging reservation, stage-result
commit and ownership commit. CAS, Git worktrees, receipts, authentication and
SQLite transactions are real. Only external boundary/isolation/adapter/coverage
and durability assumptions come from a task-scoped unit-only fixture. This tests
persistence under those assumptions; it does not certify the production source
proofs, native receiver activation or full M4 exit gate. The real CLI tests retain
the production checker and verify that staging/commit remain blocked, create no
receiver directory and activation stays disabled.

M4-NP01–04 exercise actual Windows native scope/drain binding and restart, refusal
of an unrelated empty job's proof, refusal to derive seals from synthetic status,
exact native metadata retries and detection of a missing terminal execution
journal (FR-08/12/13, INV-02/06/07/09). The store test launches its own test binary
with `--help` as a harmless temporary managed process. These are source-evidence
subcases of T-003/T-006/T-008/T-009, not complete transfer or sandbox validation.

M4-RS01–03 extend T-004/T-005/T-008 with durable artifact restore intent and
completion-receipt cases (FR-08/09/13, INV-03/05/07). The native harness kills at
both sides of the intent commit, both sides of final-result commit, after Git
worktree creation and before/after receipt publication. Exact retries verify one
reserved/restored event and preserve source content. Different operations or
trust domains cannot take reserved IDs; receipt-less targets and receiver edits
are refused without overwrite. These do not establish PREPARED transfer staging,
process fencing, power-loss durability or the complete M4 exit gate.

**T-001 — Single coordinator owner** (`FR-07`, `INV-01`): Concurrent claims and accepts on one task from 2–100 clients must produce at most one authoritative current owner and monotonically increasing epochs. Duplicate requests with the same idempotency key must return the same logical result.

**T-002 — Stale epoch** (`FR-07`, `INV-01`): After epoch advances from N to N+1, heartbeat, mutation request, checkpoint commit, and activation with epoch N must be rejected and audited.

**T-003 — No premature receiver** (`FR-08`, `INV-02`): Receiver can stage read-only artifacts pre-commit; a simulated write action before epoch commit must be denied. Activation may happen only once for a committed transfer.

**T-004 — No partial checkpoint** (`FR-04`, `FR-05`, `INV-03`): Inject crash before/after every blob write, manifest write, durability barrier, and SQLite transaction. No acknowledged checkpoint may reference a missing or unverified required blob.

**T-005 — Non-destructive restore** (`FR-09`, `INV-05`): Restore cannot overwrite the source worktree, an existing target path, unrelated user files, or preexisting Git metadata, including after retry.

**T-006 — Suspicious lease expiry** (`FR-06`, `FR-07`, `INV-06`): Kill heartbeat while leaving a child writer alive. Lease metadata may become `SUSPECT`, but automatic authority transfer must remain `BLOCKED` without strong fencing evidence.

**T-007 — External effect uncertainty** (`FR-14`, `INV-04`): Start a simulated non-idempotent deployment/migration, lose completion acknowledgement, and attempt resume. No automatic replay is allowed; the effect ledger must retain `UNCERTAIN` and the transfer must not be VERIFIED.

**T-008 — Idempotent protocol** (`FR-08`, `INV-07`): Lose responses from every mutating RPC and retry in arbitrary order. No duplicate ownership change, duplicate process activation, or second competing receiver.

**T-009 — Truthful reporting** (`FR-13`, `INV-08`, `INV-09`): Introduce missing optional context, a missing mandatory blob, and a skipped event interval. Outcomes must be DEGRADED or BLOCKED according to policy; neither may be reported as VERIFIED.

**T-010 — Provenance** (`FR-10`, `FR-11`): A fact, an agent claim, and a system inference about the same failed test remain distinguishable in every storage and response format; every cited fact resolves to evidence or an explicit evidence-unavailable marker.

## 5. Workspace fixture matrix

Use deliberately adversarial Git fixtures: committed/staged/unstaged content, deletions, renames, new untracked files, executable-bit changes, symlinks (subject to platform privileges), binary payloads, Unicode and long paths, empty files, submodules, Git LFS pointers, case-only renames, lockfiles, CRLF/LF changes, merge/conflict state, `.gitignore` exclusions, sparse checkouts, partially initialized repositories, and paths outside repo via symlink traversal.

Mandatory acceptance for *supported captured files*: content bytes and documented metadata match the manifest. Ignored data and external resources must be **explicitly categorized**, not silently asserted restored. A Unix executable bit or symlink semantic that cannot be preserved on a target platform must trigger a documented degraded/blocked decision according to whether it is required.

## 6. Crash matrix and recovery oracle

Inject a hard kill (not graceful shutdown) at every durable transition:

| Kill location | Restart oracle |
|---|---|
| Before QUIESCING persists | Previous authoritative task state retained; inspect surviving commands |
| After QUIESCING, before process drain | No automatic replacement writer; detect unsettled effects |
| During CAS blob staging | Orphan staging allowed; no published incomplete checkpoint |
| After manifest durability, before SQLite publication | Same transfer ID can publish verified root or discard orphan safely |
| After PREPARED, before receiver restore | Retry receiver stage; source remains restrained |
| During receiver worktree restore | Resume validated stage or clean **only** Baton-owned temporary worktree |
| After receiver valid, before ownership commit | Receiver has no write authority; compare-and-swap may retry |
| After ownership commit, before activation ACK | New epoch remains committed; retry activation without spawning duplicate writers |
| After activation, before first receiving tool | Resume/recover with new owner; old epoch invalid |
| During uninstall/upgrade | No silent removal of user Git contents; migration and task-state recovery work |

Run the matrix with forced I/O errors, full disk, SQLite busy contention, permission denial, corrupted CAS blob, and daemon crash during cleanup. Instrument state changes so fault injection does not depend on timing luck.

## 7. Real agent end-to-end scenarios

### E2E-A — Interrupted authenticated feature

1. Begin a real task with managed Codex in a seeded Git repo; task: implement JWT auth against provided tests.
2. Allow it to modify tracked and untracked files and execute a test that fails.
3. Request transfer only at a safe, observed boundary; retain test stdout/stderr and source-event cursor.
4. Restore in an independent worktree; commit new authority; start a fresh managed Claude Code session.
5. Observe Claude's first meaningful actions. Validate it can inspect the precise failing test and relevant edits without a human summary.
6. Finish task and evaluate correctness, repeated tool calls, and original work preservation.

**Pass:** Verified restoration; no stale-authority success; Claude can act on the real failed test; final acceptance tests pass; human did not fabricate a handoff. Measure continuity improvement separately from correctness.

### E2E-B — Uncertain deployment

Make the source invoke a fake external mutation service that commits an effect but drops its completion response. Baton must enter `BLOCKED` pending resource-specific reconciliation; it must not repeat the command. Human/manual recovery is acceptable.

### E2E-C — Competing continuation

Trigger two receiving agents simultaneously; exactly one wins. The losing receiver may keep a read-only inspection copy but cannot perform coordinator-authorized writes.

### E2E-D — Adapter degraded mode

Disable plugin, kill hook helper, block MCP, and run a vendor mode that bypasses normal hooks. Baton must accurately report reduced adapter guarantees instead of claiming fully supervised operation.

## 8. Security abuse cases

- Malicious repository content instructs the receiving model to ignore Baton restrictions: never grant new authority based on untrusted files or transcripts.
- Forged task ID, actor ID, or epoch in MCP requests: reject without capability/authorization.
- Symlink escapes and path traversal in manifest: reject; never materialize outside the planned worktree.
- Secret-like strings in environment, tool output, files, and command line: test redaction, exclusion and export policy with seeded dummy credentials.
- Stolen/expired local IPC credentials: reject; local socket/pipe authorization and file permissions remain enforced.
- Replay of old transfer accept and stale checkpoint: reject by operation identity and checkpoint/epoch binding.
- Disk-full during publish and partially corrupted compressed blobs: no VERIFIED outcome.
- Attempts to take over after network loss without source fencing: refuse automatic writer activation.

## 9. Performance benchmark suite

Use fixed, published fixtures and report p50/p95/p99, cold versus warm caches, CPU, disk, OS, daemon memory, artifact size, and test duration. Test repos of 1K/10K/100K files and changed payloads of 100 KB/5 MB/100 MB; include small-text-heavy and large-binary-heavy cases.

Track:
- Added per-event processing cost; separate agent latency from Baton overhead.
- Checkpoint time: from confirmed quiescence to published checkpoint.
- Restore time: from restore start to verified worktree readiness (not including model startup).
- Context packet size and retrieval latency; number of events discarded versus reachable by evidence tools.
- CAS deduplication ratio, compression throughput, GC correctness and storage growth.
- p95 IPC read latency, process supervision overhead and daemon idle memory.

Treat the NFR thresholds in `PRD.md` as targets subject to reproducible tuning; never invent benchmark results.

## 10. Quality-of-continuity evaluation

For a set of realistic interrupted development tasks, compare three randomized conditions:

**Baseline A:** Git state alone. **Baseline B:** Git state plus a human-authored or carefully generated written handoff. **Baton:** restored verified checkpoint plus evidence-backed context. Keep model versions, task fixtures, budgets and permissions comparable. Evaluate success with deterministic tests plus manual review where needed.

Metrics: task completion, correctness, p50/p95 time to first *correct* edit, repeated file reads/searches/commands, token usage, new regressions, manual interventions, risky duplicate side effects, and overall wall-clock time. Report uncertainty and cases where Baton performs worse. Do not claim product differentiation without this evidence.

## 11. Exit criteria by release

- **R0:** T-001 through T-010 pass in local automated suites; local E2E-A/B/C on at least one supported platform; real vendor version evidence; no severe data-loss or safety defect.
- **R1:** all R0 requirements plus full 3-OS CI coverage, install/upgrade/uninstall smoke tests, local MCP/plugin trust/config coverage and observed overhead benchmarks.
- **R2:** R1 plus authenticated cross-machine integration, partition tests, clock skew, interrupted object-sync, failed central coordinator, and revoked-device rejection. Never claim remote split-brain safety without enforced fencing across the actual shared resources.

## 12. Reporting format

Each completed milestone must report: test IDs run and result; code revision; host OS versions; adapter/provider versions; p95/p99 benchmark observations if available; known gaps; explicit `VERIFIED` versus `DEGRADED` versus `BLOCKED` scenarios; and links to anonymized evidence. A failing invariant test blocks release regardless of UI or demo quality.
