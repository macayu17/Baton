# Product Requirements Document — Baton

**Version:** 1.0 (design baseline)\
**Date:** 22 October 2026\
**Status:** Ready for implementation planning; product assumptions and stretch goals are explicitly marked.\
**Audience:** Codex implementers, maintainers, designers, early testers.

## 1. Executive summary

Baton is an independent, local-first **execution-continuity layer** for AI coding agents and collaborating developers. It preserves the *observable state* of work—Git workspace changes, completed tools and outputs, verified checks, decisions with provenance, environment manifest, and ownership epoch—so another agent can safely continue from the last recoverable boundary. It is not just a Markdown handoff, prompt memory, chat export, or session-format converter.

**Tagline:** Continue the work, not the conversation.

**Core promise:** Move an unfinished task from Codex to Claude Code (and eventually other agents and machines), with evidence-backed restoration, explicit limitations, and guarded ownership changes.

### Why now

Current Git collaboration shares committed code but not a consistent account of an agent's in-flight operations, failed attempts, environment assumptions or task ownership. Chat histories and handoff summaries can omit relevant details; naive session conversion does not make external effects safe or coordinate multiple developers. Baton bridges that execution gap.

## 2. User segments

1. **Solo multi-agent developer:** switches from Codex to Claude, or between devices, without repeating exploratory tool calls.
2. **Small development team (2–10):** transfers half-completed tasks with Git context and traceability, using different agent providers.
3. **Open-source maintainer:** hands a task between contributors and agents without copying proprietary or sensitive private sessions.
4. **Advanced local developer:** demands command-line control, offline operation and auditable restoration, not a hosted dependency.

## 3. Jobs to be done / illustrative scenarios

**U1 — Interrupted test debugging:** Codex creates and edits authentication files, runs tests, and encounters two failures. Developer hands task to Claude Code. Baton restores a fresh isolated worktree, provides evidence-linked failures, assigns a new fenced owner and Claude works on the failure rather than independently rediscovering the investigation.

**U2 — Source crash:** Machine dies after a tool starts but before completion. Baton recovers the last durable checkpoint and identifies unresolved effects. It does not invent a successful completion or replay risky commands.

**U3 — Task contention:** Two developers attempt to claim the same task. Exactly one authoritative claim wins. The loser gets a clear, actionable conflict report.

**U4 — Context loss:** Historical tool output is huge. The receiving agent receives a compact, relevant continuation packet with references to full immutable evidence. It can ask for details on demand.

**U5 — Cross-machine handoff (post-local MVP):** Developer A shares an encrypted checkpoint with Developer B using authenticated coordination and explicit authorization. Divergence and partial downloads are detected before activation.

**U6 — Same-provider continuation:** Restarting a Codex task or Claude task uses the same protocol, not a special case.

## 4. User experience and installation

The default experience is a native CLI plus background local daemon. There may be a lightweight status UI later, but no UI is required to complete the primary workflow. Users install a signed native binary through platform package managers or releases, run `baton init` in a repo, consent to agent interception, and optionally connect Codex/Claude through MCP and plugin packages. See `docs/INSTALLATION.md`.

Proposed commands (product requirements, not current software): `baton init`, `start`, `status`, `checkpoint`, `transfer`, `accept`, `recover`, `doctor`, `mcp serve`, `uninstall`. Operations must expose `--json` for machine clients and readable text for humans. Commands that change ownership, run processes, export sensitive data or upload artifacts require appropriate permission/confirmation.

### Desired primary flow

1. User installs Baton, validates prerequisites and initializes a Git repository.
2. User starts a supervised Codex task with a specific task ID and scoped worktree.
3. Baton records structured events and creates periodic safe checkpoints, without blocking ordinary tool actions unnecessarily.
4. User requests transfer to Claude; Baton quiesces source or confirms a previously safe boundary.
5. Baton captures/verifies snapshot and execution evidence, prepares a receiver workspace, and atomically transfers authority if safety prerequisites are satisfied.
6. Claude receives a bounded continuation packet and read-only evidence retrieval tools; begins work only after valid authority is established.
7. User can inspect status, event timeline, failures, context provenance, and rollback/recovery options.

**Never require the user to write a handoff summary.** Optionally accept annotations or review the automatic continuation packet, but useful progress must be possible without them.

## 5. Release scope

### R0 — Local vertical slice (must ship first)

- One local host, one Git repository/task at a time per test; independent worktrees allowed.
- Managed Codex source -> Claude Code receiver with capability-aware adapters.
- Durable SQLite event journal, process supervision, heartbeat/lease fencing.
- Safe boundary detection, content-addressed checkpoint and manifests.
- Restore tracked, staged, unstaged, deletions, and explicitly included untracked content to a *new worktree*.
- Detect missing/ignored/lfs/submodule/environment gaps rather than claiming complete restoration.
- Evidence-backed continuation packet and read-only evidence query.
- Verified / degraded / blocked outcomes; no automatic replay of unknown external mutations.
- Recovery from source/daemon/receiver crashes and duplicate requests.
- CLI plus diagnostics and comprehensive fault-injection tests.

### R1 — Installable local product

- Signed/reproducible binaries for Windows/macOS/Linux, automatic setup assistant and doctor.
- Local stdio MCP gateway, Codex/Claude plugin/skills/hook packaging where supported.
- Explicit opt-in instrumentation, export and uninstall/cleanup with preservation of user project data.
- Schema migration/version negotiation, retention controls, basic compatibility matrix.

### R2 — Team coordination (after R0/R1 reliability)

- Authenticated team service, PostgreSQL authoritative leases, encrypted chunk/blob sync, S3-compatible object store.
- Multiple machines, offline read-only/recoverable mode, force recovery with human authorization and verifiable isolation.
- Per-task scoped permissions, audit trails, task handoff notifications, conflict/reconciliation UI or CLI.

### R3 — Extensions

- Additional agents, editor integrations, richer task/dependency graph, benchmarking dashboards, optional hosted service.

## 6. Functional requirements

Use requirement IDs as test prefixes and trace them to `docs/TESTING.md`.

| ID | Priority | Requirement | Acceptance criterion |
|---|---|---|---|
| FR-01 | P0 | Initialize and identify a Git repository without altering its existing state | Initializing is idempotent and never changes tracked/untracked user files unexpectedly |
| FR-02 | P0 | Create task identities with goal, scope, actor and agent metadata | Task persisted with unique ID and versioned record |
| FR-03 | P0 | Supervise source agent lifecycle and stream observable events | Events have sequence, origin, timestamp, outcome and raw evidence pointer; interruption acknowledged distinctly from quiescence |
| FR-04 | P0 | Persist durable append-only event log | Crash recovery never treats an uncommitted event as committed |
| FR-05 | P0 | Generate consistent, immutable workspace snapshot | Byte-content restoration verified for supported file types and staged/unstaged states |
| FR-06 | P0 | Determine safe transfer boundary | Transfer blocks if process/child activity or effects cannot be established safe |
| FR-07 | P0 | Issue exclusive, monotonic fenced ownership | Concurrent claims produce at most one winner in tested coordinator-controlled paths |
| FR-08 | P0 | Commit ownership through idempotent two-phase handoff | Lost responses/duplicate accept requests don't allocate multiple owners |
| FR-09 | P0 | Restore to fresh independent Git worktree | Never clobbers source or an existing developer checkout; hash validation passes |
| FR-10 | P0 | Create evidence-backed context packet | Packet separates facts, reported intent and inference; links to event IDs |
| FR-11 | P0 | Allow receiver to query evidence rather than load full trace | Event, files, test-failure and decision access enforce task permissions |
| FR-12 | P0 | Recover after process/daemon crash | System deterministically reaches recoverable, degraded or blocked state |
| FR-13 | P0 | Show honest recovery quality | VERIFIED / DEGRADED / BLOCKED output with missing guarantees and reasons |
| FR-14 | P0 | Mark ambiguous external effects | Unknown effects are never replayed automatically |
| FR-15 | P0 | Validate source/receiver adapter capabilities | Unsupported hard guarantees prevent automated transfer |
| FR-16 | P0 | Keep secrets/private data out of checkpoint uploads by default | Redaction and explicit include/exclude policy enforced; cloud disabled initially |
| FR-17 | P1 | Provide local MCP read/control tools | Agent cannot bypass daemon authority or write without epoch+authorization |
| FR-18 | P1 | Offer plugin-assisted installation/setup | Plugins call core daemon, do not duplicate persistence or lease engine |
| FR-19 | P1 | Provide `doctor`, logs, and structured errors | Common prerequisites, permissions, lifecycle failures diagnosable |
| FR-20 | P1 | Manage retention and secure removal of Baton-owned state | Deletion is explicit and does not delete unrelated repo content |
| FR-21 | P2 | Authenticated cross-machine sync and leases | Distributed concurrency/partition fault tests pass in R2 |
| FR-22 | P2 | Detect Git/environment divergence and reconcile | Divergence blocks or requires explicit safe merge/rebase path |
| FR-23 | P2 | Audit sharing and access events | Exportable timestamped actor/activity record without unnecessary secret data |
| FR-24 | P2 | Allow ownership changes/revocation by authorized maintainers | Recovery requires fencing and isolation evidence; no blind takeover |

## 7. Non-functional requirements and preliminary performance targets

Targets are **proposed acceptance targets**, not benchmarked claims. Revise them only with profiling evidence.

- **NFR-01 Reliability:** zero acknowledged checkpoints with failed integrity validation; zero accepted stale epochs in coordinator-controlled operations.
- **NFR-02 Performance:** p95 local coordinator read queries <50 ms under representative test load; event recording p95 added processing overhead <10 ms excluding upstream agent latency; benchmark on fixed hardware.
- **NFR-03 Checkpoint speed:** p95 incremental checkpoint for a warmed 100 MB typical Git workspace with <=5 MB changes <2 seconds excluding active tool drain and network; record cold/warm/storage differences.
- **NFR-04 Restore speed:** p95 restore into prepared local worktree <5 seconds for the same fixture; measure separately from environment provisioning and model start.
- **NFR-05 Resource usage:** idle daemon target <75 MB RSS; report by OS, build mode and active adapters rather than hiding variance.
- **NFR-06 Portability:** Windows 11, current macOS, two representative Linux distros; path, permission and process tree tests.
- **NFR-07 Offline:** R0/R1 fully local operations require no Baton account/cloud connectivity; providers may separately need their own authentication/network.
- **NFR-08 Compatibility:** versioned manifest and explicit adapter capability reporting; forward-incompatible checkpoints must fail with migration guidance.
- **NFR-09 Security:** least privilege, redaction, opt-in monitoring, localhost-only daemon by default, no remote bind by accident.
- **NFR-10 Observability:** event correlation IDs; logs distinguish observation, claim, and inference; no credential leaks in default logging.

## 8. Explicit product non-goals

- Transfer hidden chain-of-thought, KV-cache or another provider's internal model state.
- Guarantee exact same future agent output or zero repeated tokens.
- Fully replay running processes, network connections or GPU state.
- Automatically solve or merge Git conflicts without user-accepted policy.
- Promise exactly-once execution of arbitrary external effects.
- Replace Git, CI, issue trackers or coding-agent CLIs.
- Build a full multi-user cloud IDE or new foundation-model runtime.
- Use MCP or plugins as a security boundary or sole process-supervision mechanism.

## 9. User-facing status and transparency

**VERIFIED:** integrity confirmed, designated state restored, valid epoch authority, no unresolved operations relevant to automatic continuation. Does not mean the new model has identical internal cognition.

**DEGRADED:** restorable with known missing environmental/contextual pieces and no known unresolved unsafe effect. Requires explicit developer acknowledgement before continuing; do not display as success without qualifications.

**BLOCKED:** ownership ambiguous, missing/corrupt required blobs, unsafe side effects unresolved, uncontrolled active processes, or unsupported adapter safety capability. No automatic continuation.

All statuses must surface the reason codes and recommended corrective actions. A claim like 'Codex considered the feature complete' is not proof; a passing test with immutable output is stronger evidence.

## 10. Success metrics

**North-star metric:** percentage of interrupted tasks that the receiving agent completes successfully without a human-written handoff, without violating safety conditions.

Track: completion rate, redundant file reads/tool calls, recovery-token usage, time to first correct action, regressions, checkpoint/restore overhead, number of manual interventions, and frequency of BLOCKED/DEGRADED classifications. Compare against (A) Git-only and (B) Git + detailed manual handoff baselines, with randomized tasks and blinded evaluators where practical. Do not optimize context savings at expense of correctness.

## 11. Release acceptance scenario

A developer supervises Codex on a repository implementing authentication. Codex changes several files, leaves uncommitted modifications and one untracked test, runs a test and observes a failure. The source stops at a safe verified boundary. Baton snapshots the *real* state, records evidence and task intention as separate provenance classes. A second, isolated Claude session restores the source worktree, acquires a new epoch, reads the failing-test evidence and resumes. The old owner cannot pass enforced mutation gates. If coordinator crashes during any transfer stage, retry is idempotent and no user workspace is lost. If external effects are ambiguous, transfer is correctly BLOCKED instead.

## 12. Risks and mitigations

- **Incomplete agent observability:** capability gates and fail-closed transfer; first support managed launcher only.
- **Background processes escape supervision:** process group/Job Object containment, path-scoped isolation, explicit unresolved-process detection.
- **Secrets in command output/files:** redaction, encrypt at rest, no upload by default, safe artifact allowlist/denylist and consent.
- **Plugins/hook behavior changes:** adapters behind compatibility layer; pin versions for tests and maintain documented matrix.
- **Task handoff context inaccurate:** distinguish verified facts, agent opinions and inference; link to underlying evidence.
- **Concurrent Git modification:** isolated worktrees + authoritative task ownership, conservative conflict handling.
- **External side effects:** separate effect ledger and explicit reconciliation policy; no unsafe replay.
- **Storage growth:** incremental CAS blobs, retention policy and GC retaining referenced roots.

## 13. Open questions (do not block R0)

- Should R2 provide private peer-to-peer sync before a hosted coordinator? Proposed default: centrally coordinated leases, optional self-hosting, because authoritative fencing and audit are simpler to reason about.
- Should agent continuation packets use local lexical search, embeddings or both? Default: deterministic indexed evidence first, semantic retrieval as an optional future enhancement.
- Should Baton attempt raw session-format import? Default: optional enhancer, never the authoritative state mechanism.
- Should checkpoints include runnable container/VM snapshots? Default: not for R0; future isolated runner supports stronger fidelity at increased cost.
- Licensing/monetization: defer until core transfer behavior is proven; consider open-source engine plus optional hosted team coordination.

## 14. Related documents

See `AGENTS.md`, `docs/ARCHITECTURE.md`, `docs/PROTOCOL.md`, `docs/INTEGRATIONS.md`, `docs/INSTALLATION.md`, `docs/SECURITY.md`, `docs/TESTING.md`, `docs/ROADMAP.md`, and `docs/DECISIONS.md`.
