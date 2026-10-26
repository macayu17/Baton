# Baton — Implementation Roadmap for Codex

**Status:** Proposed implementation order. **Goal:** shipping a correct local vertical slice before installation conveniences or distributed infrastructure.

## How Codex should use this roadmap

Work **one milestone at a time**. Before coding: identify relevant `FR-XX`, `INV-XX`, test cases from `docs/TESTING.md`, and affected boundaries. Implement the smallest end-to-end behavior that can be tested. After coding: run tests, show objective evidence, update decisions and compatibility notes, and mark the milestone complete only if the exit gate passes. Do not treat optional components as a reason to postpone core correctness.

## M0 — Repository and contract baseline

**Implementation status (22 October 2026):** local baseline implemented and tested
on Windows GNU; three-OS exit gate pending. See `M0-BASELINE.md`. The user then
authorized M1 development while this validation remains pending (ADR-017).
Do not mark the M0 exit gate passed without native OS evidence.

**Goal:** establish a buildable Rust workspace and an executable test harness without feature claims.

Scope: Cargo workspace boundaries; error taxonomy; schema version policy; OS feature flags; Git fixture helpers; CI lint/build/unit tests; formatted diagnostic conventions. Use docs to design modules, not a speculative microservice fleet.

**Exit:** three OS builds, deterministic tests run, canonical errors and requirements traceability documented. No daemon or agent claims yet.

## M1 — Durable tasks, epochs and event log

**Implementation status:** experimental metadata slice implemented; disabled by
default. See `M1-IMPLEMENTATION.md` for tests, authority limits, and remaining
validation. M1 reservations do not activate writers. Full transfer/activation
parts of T-001/T-002/T-008 still depend on M3/M4; no complete R0 gate is claimed.

**Targets:** `FR-01`, `FR-02`, `FR-04`, `FR-07`, `FR-08`; `INV-01`, `INV-07`.

Scope: per-repo metadata enrollment without modifying user files; SQLite WAL state; event sequence and actor identity; transactional task claims; monotonic epochs; compare-and-swap ownership; heartbeat and `SUSPECT` classification; idempotency ledger. Do not enable takeover on mere expiration.

**Exit:** tests T-001, T-002 and T-008 pass with concurrency and crash injection.

## M2 — Content-addressed checkpoint engine

**Implementation status:** local experimental forensic capture/restoration slice
implemented; disabled by default. See `M2-IMPLEMENTATION.md` and ADR-018. T-004/T-005
subcases pass on Windows GNU, but native three-OS validation, Windows directory
durability, the full workspace matrix and representative performance remain open.
No VERIFIED checkpoint/continuation or complete M2 exit gate is claimed.

**Targets:** `FR-05`, `FR-09`, `FR-16`; `INV-03`, `INV-05`.

Scope: snapshot capture policy; staged/unstaged/untracked paths, metadata and exclusions; immutable manifests; BLAKE3 hashes; zstd; staging and durable publication; new Git worktree restoration; no overwrite; CAS garbage collection only with reference-awareness. Keep snapshots wholly local.

**Exit:** T-004, T-005 and workspace fixture matrix pass; checkpoint timing measured honestly.

## M3 — Native process supervision and safe boundaries

**Implementation status:** experimental process-control and effect-ledger slice
implemented; disabled by default. Windows GNU fake-agent T-006/T-007 subcases
pass, including actual supervisor hard kill. See `M3-IMPLEMENTATION.md` and ADR-019.
Full M3 gate remains open: Unix containment, native OS validation, vendor event
coverage and a production safe-boundary proof are not established. M4 must not
promote these records or adapter capability claims directly into write authority.

**Targets:** `FR-03`, `FR-06`, `FR-12`, `FR-14`, `FR-15`; `INV-04`, `INV-06`, `INV-09`.

Scope: managed process tree per OS (Windows Job Objects / Unix process group isolation where suitable); event normalization; action/effect ledger; graceful stop, uncertain child detection; crash recovery; safe quiescence predicate; capability negotiation. A killed client must not be mistaken for a safely drained process.

**Exit:** T-006, T-007 and crash matrix pass in fake-agent harness; unresolved external effect is BLOCKED.

## M4 — Transfer state machine and local CLI

**Implementation status:** experimental implementation now includes the complete
Windows simulated transfer tracer: native fixture readiness, first-tool admission
and completion, successive A -> B -> A handoff, retained receiver edits, and
coordinator crash/retry cases. Durable CLI/daemon, staging, ownership CAS and
recovery use the existing private native boundaries. Positive transfer tests
still require explicit unit-only boundary assumptions; production PREPARED and
CLI activation remain disabled. ACTIVE fixture metadata remains
BLOCKED/RECEIVER_ADAPTER_UNVALIDATED. The bounded Windows simulated gate passed
with 131 all-feature tests (zero failed), including actual coordinator kills;
see `M4-IMPLEMENTATION.md` for evidence and limits. Native three-OS, real-vendor, isolation
and durability release gates remain open; M5/M6 must establish their own evidence.

**Targets:** `FR-08`, `FR-09`, `FR-12`, `FR-13`; `INV-02`, `INV-07`, `INV-08`.

Scope: QUIESCING → PREPARED → RESTORING → COMMITTED → ACTIVE; persistent transfer IDs; separate receiver staging and activation; CLI commands with `--json`; checkpoint verification report and actionable recovery messages; no ambiguous success state.

**Exit:** T-003, T-008, T-009 and local end-to-end simulated transfer work across deliberate coordinator crashes.

## M5 — Real Codex source adapter

**Implementation status:** experimental supervised stdio handshake implemented.
Installed Codex 0.164.0 completes initialize/initialized and native stop using a
private temporary home; schema-only discovery of 0.163.0 remains historical.
Malformed messages, EOF, timeout, cancellation and transport/environment binding
are tested. Bounded turn and command/file-change lifecycle metadata now persists
with run-scoped dedup identities, reported exit facts, hash validation and explicit
gap/uncertainty handling. Schema 13 adds five passive item metadata types and
cancel-only command/file approval handling, with separate host intent/write and
vendor resolution evidence. No execution grant is implemented. Schema 14 adds
durable turn-interrupt intent/write/reply correlation, with live
retry suppression and terminal-turn/native-drain separation tested on synthetic peers.
Coverage is partial; unsupported messages stop capture. No real model turn, source-work capture or
source safety gate is certified. See `M5-IMPLEMENTATION.md` and ADR-028–031.

The source-admission tracer now runs the actual installed Codex against a
loopback mock Responses provider. Candidate profiles can suppress observed shell,
image and goal tools, but the strict Baton-only inventory probe still finds
request_user_input and fails. This is test-only compatibility evidence, not a
real model or source grant. Schema 15 adds input blocking and dynamic failure
responses, durable identity/payload binding and negative native callback probes.
The native Session now has a create-only host operation backed by fresh tool
permits, held directory identity and byte readback; existing files and ambiguous
partial results are preserved. Schema 16 adds opt-in baton_create_file_v1 callbacks
through that host operation, with intent/completion/response/resolution separated
and synthetic native peers tested. Native Codex registration/positive callbacks,
complete source capture and real source-work proof remain missing. See
ADR-032–035. Schema 17 now supplies explicit experimental initialization and
audited ephemeral thread registration through Source, including an actual native
registration test without a model request. Startup effects remain Unknown and
block subsequent writes; validated startup policy, native positive work and
complete capture/safe stopping remain open. See ADR-036; stay within M5.

Schema 18 now checks execution summaries and observed-run cached replies against
journal-bound post-transition state hashes. This closes reproduced corruption
paths through the uncertain-startup gate without settling startup effects or
opening source activation. See ADR-037 and M5-SG01–03.

Schema 19 adds pre-registration config/read with audited summary and rejection of
reported executable service configuration. Native M5-SP01 demonstrated that a
configured MCP child can run under read-only thread metadata before any model
turn. The new rejection gate does not freeze configuration or settle Unknown
startup; validated isolation and real source-work proof remain open. See ADR-038.

Launch-v2 now includes main-executable bytes and pre-spawn identity/digest checks,
with Windows file sharing held through the native scope lifetime. This closes
same-path admission drift without claiming dependency or startup confinement.
Schema remains 19. See ADR-039/M5-B01–04; M5 is still the current milestone.

An opt-in native containment tracer now compares an uncontained Rust fixture with
Microsoft MXC base-container policy. Allowed reads and denied file/network access
have bounded Windows evidence without host DACL/BFS fallback. This is not a
production backend or Codex isolation proof; policy binding, lifecycle/crash
handling and actual source work remain open. See ADR-040/M5-NC01–04.

**Targets:** `FR-03`, `FR-06`, `FR-10`, `FR-15`.

Scope: supervised Codex App Server; feature-detected thread/turn/item events; approvals; tracked event cursor; graceful interrupt and independent quiescence evidence; reproducible source-work fixtures; explicit version compatibility table. Do not mine inaccessible hidden reasoning or undocumented private files for authoritative facts.

**Exit:** real Codex session capture and safe stopped boundary under pinned tested versions, plus degraded-mode tests when events are missing.

## M6 — Real Claude receiver adapter and evidence context

**Targets:** `FR-10`, `FR-11`, `FR-13`, `FR-15`.

Scope: managed Claude Code session; capability-tested hooks/SDK; read-only preparation; continuation packet with provenance and evidence references; post-commit authorization; reverse adapter architecture kept possible but not required immediately.

**Exit:** E2E-A on the primary dev OS, T-010, and explicit report of what context was reconstructed versus restored byte-for-byte. **This is the first real product demo.**

## M7 — Harden R0 release

**Targets:** all P0 requirements `FR-01`–`FR-16`.

Scope: finish adversarial suite; verify invariant preservation across restarts; handle corrupted state, partial effects and stale owners; benchmark p95/size; spell out unsupported OS/agent modes. Do not promise cross-machine or arbitrary unmanaged session takeover.

**Exit:** R0 gate of `docs/TESTING.md` passes; usable internal alpha release, no unproven VERIFIED states.

## M8 — Installability, MCP and local plugins (R1)

**Targets:** `FR-17`–`FR-20`.

Scope: signed native releases; per-OS packaging and user-scoped service installation; opt-in enrollment and reversibility; `baton doctor`; stdio MCP gateway to local daemon; Codex and Claude skills/plugin packages, respecting each host's trust and hook rules; safe upgrades, retention and removal.

**Exit:** three-OS installation/upgrade/uninstall smoke tests; same R0 transfer safety with plugins disabled and MCP unavailable; MCP cannot circumvent the core authority checks.

## M9 — Distributed team mode (R2)

**Targets:** `FR-21`–`FR-24`.

Scope: central PostgreSQL source of truth; authenticated actors and devices; encrypted CAS artifact sync; device authorization; lease/fencing in cross-machine topology; explicit partition/clock-skew recovery; reviewer approvals and audit. Avoid peer-to-peer split-brain promises where shared writes cannot be fenced.

**Exit:** R2 partition/security gates; A-to-B machine real transfer; no cross-user artifact leak; explicit degraded cases for uncontrollable external resources.

## M10 — Optional ecosystem expansion

Potential additions: Claude → Codex reverse transfer, same-provider optimized resume, richer task dependency graph, IDE UI, local web status page, optional session converter, remote deployment environments, self-hosted team coordination, more adapters. Each addition must preserve R0/R1 invariant tests.

## Milestone dependencies

`M0 → M1 → M2 → M3 → M4 → M5 → M6 → M7 → M8 → M9` is the recommended critical path. Independent adapter spikes or CLI UX prototypes may occur in parallel, but never become sources of truth or bypass `M3/M4` safety gates.

## Immediate first Codex assignment

Read `AGENTS.md`, `PRD.md`, `docs/ARCHITECTURE.md`, `docs/PROTOCOL.md`, `docs/SECURITY.md`, and `docs/TESTING.md`. Begin **M0** only. Present a short module/testing plan, establish the Rust workspace/CI/test harness, run tests, and report which M0 exit conditions were met. Do not implement cloud sync, plugin publishing or frontend UI in the first iteration.

## Decisions requiring product-owner review

Before R1 public distribution: licensing, telemetry opt-in, supported default OS builds, package signing identity, terms for third-party session-data processing, encrypted capture policy and retention defaults. Before R2: trust model for device identity, remote takeover policy, key distribution/revocation, hosted vs self-hosted server and data residency. Continue local M0–M7 without needing these decisions when feasible.
