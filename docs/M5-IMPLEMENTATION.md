# M5 plan and compatibility evidence

Status: experimental supervised handshake, bounded lifecycle capture and cancel-only
approval handling and bounded turn interruption implemented,
25 October 2026. The
installed Codex 0.164.0 completed a local handshake under native control. No real
model turn, source-work capture or M5 safety gate is certified. M4's bounded
Windows simulated gate is recorded in M4-IMPLEMENTATION.md; its missing
production proofs remain closed.

## Implementation plan

| Increment | Deliverable | Requirements/invariants | Planned tests |
|---|---|---|---|
| 1 | Version-pinned public schema discovery and bounded stdio handshake under native supervision; no model turn | FR-03/15, INV-06/09 | M5-C01 schema method/field compatibility; M5-C02 initialize/initialized correlation, unsupported/malformed response and timeout |
| 2 | Task/run/thread/turn/item-bound observation normalizer with explicit observed cursor and durable dedup identity | FR-03/06/10/15, INV-04/07/08/09 | M5-E01 starts/completions/errors; M5-E02 duplicates, missing starts, wrong binding, reconnect/unknown event gaps; M5-E03 claim versus fact and redacted evidence |
| 3 | Approval handling and graceful interruption separated from independently observed native drain | FR-03/06/15, INV-02/04/06/09 | M5-A01 no automatic approval, unsupported request/host failure denial; M5-I01 interrupt acknowledgement versus terminal turn versus native scope drain |
| 4 | Managed synthetic source-work fixture, crash/reconnect handling and explicit compatibility/quality report | FR-03/06/10/15, INV-01–09 as applicable | M5-S01 pinned real source session; M5-S02 dropped events, uncertain effect and supervisor hard kill; T-006/T-007/T-009 subcases |

Tracer: a supervised local App Server initialize/initialized exchange that binds
runtime metadata without starting a thread/turn, reading private sessions or
granting writer authority. First write failing bounded protocol tests, then the
minimum Rust client using existing platform/supervisor/protocol/store modules.
Do not create a second daemon, SDK runtime or shell/MCP endpoint. Extend existing
native process I/O only where the tracer requires it and cover process loss.

For later real sessions use a new synthetic repository, explicit network/data
policy and the actual installed sandbox/approval capabilities. No user repository
or transcript upload is authorized by schema discovery. A successful initialization
or reported vendor version cannot establish event completeness, source fencing,
safe effect settlement, sandbox enforcement or the M5 exit gate.

## Read-only discovery performed

The installed npm launcher reports `codex-cli 0.163.0`; its package metadata
also says 0.163.0. `app-server --help` exposes default stdio and local
`generate-json-schema --out`. Schema generation succeeded without starting a
model turn. Generated artifacts are temporary local vendor schemas, not checked
into the repository. They include ClientRequest, ServerRequest,
ServerNotification, TurnInterruptParams, ItemStartedNotification,
ItemCompletedNotification and TurnCompletedNotification.

The fetched [official App Server documentation](https://developers.openai.com/codex/app-server)
describes newline-delimited stdio, initialize then initialized, version-specific
schema generation, item start/completion notifications, server-initiated approval
requests and separate turn interruption/completion. This is interface discovery,
not observed session behavior. The inspected event descriptions do not establish
a durable resumable global cursor; the client must not claim one. Missing coverage
after disconnect remains an explicit gap until independently demonstrated.

At the 23 October discovery increment, no app-server service, thread, model turn, approval, authentication flow or real
Codex/Claude transfer was started by these checks. No runtime compatibility gate
is passed yet. Next: M5-C01/C02 and the supervised local handshake tracer.

## Supervised handshake increment — 25 October 2026

The first M5 tracer now uses the new experimental Rust library
`baton-adapter-codex`, existing audited Session launch and the same native process
scope. The library has no default runtime surface or CLI/MCP endpoint. It sends
initialize with stable API mode and explicit gateway OAuth, accepts only the
correlated bounded reply, then sends initialized. It exposes only checked platform
family/OS; vendor user-agent, home path and error payload are discarded. It sends
no thread/start, turn/start, approval decision, prompt or repository contents.

LaunchSpec now explicitly selects piped stdin/stdout; stderr remains null. The
choice participates in the launch fingerprint, and pipe ownership can be taken
once. Explicit environment entries are bounded, canonicalized by key and reject
case-insensitive duplicates; values participate in the fingerprint but are not
stored as a launch recipe. Environment inheritance remains cleared. The old
null-stdio/empty-environment fingerprint is unchanged. The native probe uses a
new private home under Baton-owned temporary state with CODEX_HOME/HOME/USERPROFILE,
Windows SystemRoot where needed, and analytics.enabled=false. It does not read
or alter the normal user configuration. Native startup networking itself was
not instrumented; no network-isolation guarantee is inferred from these settings.

The handshake has a caller-selected deadline greater than zero and at most
30 seconds, a 64 KiB JSONL frame bound, strict reply decoding, ID correlation and
platform checks. EOF, invalid/truncated input, unexpected notifications, duplicate
fields, both result/error, unknown fields or unsupported shapes refuse initialization.
Diagnostics are ADAPTER_PROTOCOL, ADAPTER_TIMEOUT or ADAPTER_REJECTED, with no raw
vendor text. Existing I/O/native/storage diagnostics remain available.

Failure and cancellation of connect terminate the owned native scope and attempt
to record SupervisorLost. Ordinary failures surface cleanup/journal errors;
cancellation cleanup is necessarily best-effort if storage fails. Neither path
returns launch permission or replays a request. The cancellation test waits until
the peer received initialize before dropping the future. Successful callers must
use stop to journal the final boundary; dropping a live Source only inherits the
existing Session native teardown and cannot by itself promise a committed final
journal. This host lifecycle limitation must be addressed before long-lived turns.

The synthetic peer is an actual managed child. M5-C02 covers valid/invalid replies,
correlation, frame bounds, EOF and timeout; M5-C03 covers native pipe ownership,
fingerprint binding and explicit environment; M5-C04 covers cancellation with
exclusive-handle release on Windows. M5-C05 is an explicit opt-in installed-vendor
test, separate from synthetic fixtures. Git plumbing now requests CREATE_NO_WINDOW
on Windows; this does not control unrelated applications' plugin refresh processes.

| Codex version | Evidence on this host | Supported claim |
|---|---|---|
| 0.163.0 | Public schema/help discovery on 23 October | Schema discovery only; no runtime source session tested |
| 0.164.0 | Native --version, regenerated public schemas, managed initialize/initialized and stop on 25 October | Windows local handshake only; no model/tool/event-coverage guarantee |

The 0.164.0 generated InitializeResponse still requires codexHome, platformFamily,
platformOs and userAgent. Its generated InitializeCapabilities includes
explicitGatewayOauth; that option was verified in the installed schema, not
assumed from the overview page. Rechecked the
[official App Server documentation](https://learn.chatgpt.com/docs/app-server).
The actual native test passed in 0.78 seconds with no model session directory.
The absence of a session directory is supporting evidence; the client's only
outbound protocol messages are initialize and initialized by construction.

Files added: adapter Cargo manifest, library/client, native handshake tests and
synthetic protocol peer; platform process-I/O tests. Files changed: workspace
manifest/lockfile; platform LaunchSpec/native stdio; supervisor pipe ownership and
connection-loss handling; protocol diagnostics/tests; background Git helpers;
README, roadmap, integration/protocol/security/architecture/testing/reference and
decision records. Tokio remains pinned at 1.54.1 with its process feature enabled;
no new third-party library or Node/Python runtime requirement is introduced.
ADR-028 records the transport/environment and lifecycle limits.

Final verification on Windows 10.0.26300.0 / Rust 1.98.0 GNU /
Git 2.51.0.windows.1; no Git revision is available because this workspace has no
`.git` directory:

- `cargo +stable-x86_64-pc-windows-gnu test --workspace --all-features --locked`:
  **139 passed, zero failed**, exit 0. Six ignored entries consist of five
  subprocess helpers invoked by parent tests and the separately opt-in real
  vendor test. Evidence: `%TEMP%/baton-m5-handshake-workspace-tests.log`.
- The explicit installed-vendor command with BATON_CODEX_BINARY and
  `--test handshake installed_codex_initializes_in_isolated_home_without_model_turn
  -- --ignored --exact --nocapture` passed **1/1**, exit 0. Evidence:
  `%TEMP%/baton-m5-codex0164-handshake.log`. It validates the returned initialization
  response and the client's flushed initialized notification; that notification
  has no peer acknowledgement and is not proof of later request handling.
- Default workspace tests passed **25/25**, zero failed. Evidence:
  `%TEMP%/baton-m5-handshake-default-tests.log`. The cancellation case was also
  rerun directly after replacing a timing delay with a received-message marker;
  it passed. The initial test-first ExecutionNotEnabled failure and missing pipe
  APIs were resolved, not skipped. A fixture PathBuf borrowing error was corrected.
- Formatting, default/all-feature all-target Clippy with warnings denied, and
  all-feature/all-target workspace build passed with the locked dependencies.
- Platform-only Linux x86_64 and macOS arm64 all-target/all-feature checks passed
  with `--features blake3/pure`. They do not type-check the entire adapter/store
  dependency graph or prove native runtime behavior. Full foreign C toolchains,
  native Unix execution and native three-OS CI remain unverified.
- No live Baton/fixture/vendor-probe process or repository database/log remains.
  The test directories and native vendor home are synthetic local fixtures;
  normal user configuration and credentials were not enrolled or modified.

The handshake slice is functional and remains disabled by default. M5's exit
gate has **not** passed: there is no real source model turn, durable vendor item
stream, verified stopped source boundary, source-work checkpoint or useful
Codex-to-Claude continuation yet. Production transfer remains BLOCKED.

## Durable lifecycle metadata increment — 25 October 2026

Plan: normalize supported turn/command/file-change boundaries, bind opaque vendor
identities to the actual RunBinding, retain reported exit facts, and test duplicate,
orphan, conflicting, wrong-stream, interrupted and corrupt evidence cases
(FR-03/04/10/12/14/15, INV-04/07/08/09; M5-E01–05). This is evidence plumbing within
M5, not its real-source or safe-boundary exit gate.

`Source::capture_next` consumes one bounded native JSONL notification under a
positive deadline of at most 30 seconds. Its supported subset is turn/started,
turn/completed and commandExecution/fileChange item/started/item/completed. It
requires an explicitly selected thread and adapter/version. Only bounded opaque
IDs, lifecycle kinds, reported outcome and optional exit code persist. Raw command,
stdout/stderr, agent text and reasoning content are omitted, not represented as
retrievable raw evidence. A vendor-reported zero exit is not independent proof
that tests passed or that side effects are settled.

The existing RunObservation/ExecutionEvent journal now carries AdapterReported
and AdapterGap. Adapter stream headers cannot rebind an existing run; turn/item
correlation is derived from prior journal entries. Missing predecessors create a
sticky gap. An orphan completion stores its reported outcome and an uncertain
effect without inserting a fake observed-start event. All current vendor tool
effects use Unknown classification, including successful file changes, until a
concrete independent policy/reconciler exists. Turn completion never proves
process drain, grants authority or settles pending tools.

Run-scoped BLAKE3-derived operation/effect IDs are deterministic across metadata
retries. Exact duplicate normalized facts reuse the original source sequence and
cached result; changed facts under that key are audited conflicts. The retry
result is historical. Task evidence hashes bind each new adapter execution event;
reads and cached acknowledgements check the binding. The scalar source cursor
is local accepted-observation order, not a vendor-issued durable global cursor
or proof that no event was missed. Correlation/dedup currently read the existing
journal; representative throughput is unbenchmarked.

Failed/cancelled capture records a gap and loss where possible and terminates its
owned scope. A test exposed that Session could record SupervisorLost on a conflict
before capture cleanup could write AdapterGap; SupervisorLost now itself marks the
unconfirmed event tail as a sticky gap. No old owner resumes. Evidence corruption
refuses reads/retries; native teardown still works on ordinary Source drop even
when a final audit cannot be committed. Successful live hosts must still stop
explicitly; general durable post-crash reconciliation is not replaced by Drop.

State schema 12 preserves/refuses schemas 1–11 without rewriting. Envelope version
stays 0.1.0. No new record field is silently defaulted into old state. Adapter facts
are associated with the authenticated current task/run/epoch/lease, not trusted
because a vendor ID or JSON says they are valid. Local IPC still denies supervisor
and adapter observations. ADR-029 records these choices and limitations.

Tests use actual managed native peers emitting synthetic notifications, real
SQLite commits, reopen/retry, controlled timeout/cancellation and evidence damage.
These peers do not execute Codex commands or prove reported outcomes true.
Protocol tests cover unknown effects, orphan facts and stream correlation; parser
tests cover failed exit facts, omitted sensitive text and unsupported wire shapes.
The generated Codex 0.164.0 schemas were inspected for required timestamps and
command/file statuses. Real vendor runtime evidence remains the earlier handshake.

Files added: protocol adapter metadata/types and tests; Codex wire normalizer.
Files changed: run transition/origin logic, store event validation/dedup reads,
schema/identity/version tests and cached acknowledgements; supervisor observation
allowlist; native source capture/cancellation guard, fixtures/tests and manifests/
lockfile; current implementation, protocol, security, architecture, integration,
testing, roadmap, reference and decision records. Existing BLAKE3 is reused; no
dependency version upgrade, frontend, MCP, plugin or cloud component is added.

Verification on Windows 10.0.26300.0 / Rust 1.98.0 GNU / Git 2.51.0.windows.1;
there is no Git revision because this workspace has no `.git` directory:

- All-feature workspace run: **147 passed, zero failed**, exit 0. Its six ignored
  entries are five subprocess helpers invoked by parent tests and the separate
  opt-in vendor probe. Evidence: `%TEMP%/baton-m5-events-workspace-tests.log`.
- Final focused adapter/protocol run: **31 passed, zero failed**, including the
  extra wire-shape test. The new native QUIESCING case then passed **1/1**:
  a new turn is retained, recovery is forced, epoch retained and child stopped.
  These two additional tests were added after the full-run executables were built
  and are not counted in its 147. Evidence:
  `%TEMP%/baton-m5-events-final-focused-tests.log` and the explicit
  `adapter_turn_after_quiescing_cannot_bypass_source_freeze -- --exact` run.
- Default workspace tests: **28 passed**, exit 0. Evidence:
  `%TEMP%/baton-m5-events-default-tests.log`.
- Installed Codex 0.164.0 initialization-only probe: **1/1 passed** again, exit 0,
  in 5.22 seconds under concurrent load. This is not a latency benchmark or
  real lifecycle-stream acceptance. Evidence:
  `%TEMP%/baton-m5-events-codex-probe.log`.
- Formatting, default/all-feature all-target Clippy with warnings denied and
  all-feature/all-target build passed with --locked. Test-first failures, a
  fixture lint, checked SQL integer conversion and the missing-gap regression
  were resolved without suppression.
- Platform-only Linux x86_64 and macOS arm64 checks passed with blake3/pure.
  Full foreign adapter/store builds and native Unix runtime remain unverified.
  Transport deadlines bound I/O waiting; synchronous transactions and cleanup
  may take additional time. Event throughput/p95 remains unbenchmarked.
- No local-control observation bypass, raw output logging, command replay,
  new writer grant or upload path was added. No live Baton/fixture/probe process
  or repository database/log remains. Three-OS, durability, privacy and real
  transfer gates remain open. M5's exit gate has not passed.

The schema-13 increment below extends passive coverage and cancellation policy.
Real source writer admission, interruption, source-work evidence and the M6/value
comparison remain outstanding. Production transfer stays BLOCKED.

## Passive metadata and cancel-only approvals — 25 October 2026

Plan: implement only the passive metadata and cancellation portion of increment 3,
reusing the existing native pipes, authenticated journal and effect ledger. Keep
execution grants disabled. Validate the narrow end-to-end cancellation tracer
before broadening real source-work support (FR-03/06/12/14/15,
INV-04/07/08/09). This is not the complete approval or interruption milestone.

| Test ID | Concrete check |
|---|---|
| M5-A01 | cancellation_requires_intent_and_has_host_provenance: intent precedes written phase; typed IDs and origins |
| M5-A02 | approval_is_identified_and_passive_text_is_omitted; approval_ids_and_scopes_are_strict_and_reply_is_only_cancel: supported methods, scope, IDs, exact cancel response |
| M5-A03 | approvals_are_cancelled_with_distinct_host_and_vendor_evidence: actual native command/file requests, numeric/string IDs, planned/written/resolved phases and uncertain effects |
| M5-A04 | storage_failure_retains_only_committed_cancellation_phases: failure before intent and after intent commit, no fictitious written evidence |
| M5-A05 | unsupported_approval_requests_close_scope_without_a_grant: unsupported method and no cancel decision |
| M5-A06 | cancellation_during_quiescing_does_not_revive_a_writer: epoch and QUIESCING retained, no effect settlement |
| M5-A07 | resolved_request_id_reuse_is_refused_without_a_second_reply: refuse ambiguous reuse and mark loss/gap |
| M5-P01 | passive_completion_without_start_is_a_gap_and_ids_are_bounded; wire omission test |
| M5-P02 | passive_lifecycle_is_retained_without_text_or_effects: all five supported passive types, no tool effects |

Architecture: no separate vendor database or service. Schema 13 adds passive
lifecycle kinds, typed RPC request IDs and cancellation phases to the existing
versioned execution journal. HostReported planned intent commits before writing
cancel; HostReported written evidence follows flush. AdapterObserved resolution
uses the saved plan's turn/item scope. The resolution alone cannot prove receipt
of this host's reply or absence of a side effect. Unknown tool effects remain
UNCERTAIN. Passive item text/content/reasoning is discarded, not reconstructed.
No positive approval, model turn, shell replay or execution permission is added.

Changed facts under a request ID conflict. Resolved-ID reuse is unsupported and
stops capture before another reply. Unsupported methods/decisions or malformed
bindings fail closed. Failure/cancellation stops the native scope and records
gap/loss when storage permits. If storage/clock fails, last committed metadata
may remain RUNNING even though teardown stopped the owned child; do not infer a
durable stopped boundary or adopt that process. Only committed phases are facts.
The I/O deadline excludes synchronous transactions and cleanup. Hosts must still
explicitly stop a successful Source to journal the final boundary.

Compatibility: generated Codex 0.164.0 approval and passive schemas inspected;
optional availableDecisions is feature-detected and must allow cancel when
present. Schemas 1–12 are preserved/refused without migration. Envelope version
remains 0.1.0; ADR-030 records the material schema/provenance choices. Existing
dependencies are reused without version changes. No user session or repository
was sent to a provider; all approval/passive traces use synthetic native peers.

Files added: protocol approval tests. Files changed: protocol adapter/execution
types and origins; store schema/version checks, full-binding event reads and
event-origin verification; Codex normalizer/client/native fixture/tests; README,
historical report current-version headers, architecture/protocol/security/testing,
integration/roadmap/reference and decision records. No frontend, distributed
backend, MCP or plugin is added.

Verification: Windows 10.0.26300.0, Rust 1.98.0 GNU, Git 2.51.0.windows.1;
workspace has no `.git` directory, so no revision hash is available. Commands use
`cargo +stable-x86_64-pc-windows-gnu` and `--locked`:

- `fmt --all -- --check`: passed.
- `clippy --workspace --all-targets --all-features -- -D warnings` and default
  all-target Clippy: passed, exit 0.
- `build --workspace --all-targets --all-features`: passed, exit 0.
- `test --workspace --all-features`: **159 passed, zero failed**, exit 0.
  Six ignored entries are five subprocess helpers invoked by parent tests and
  the separate opt-in installed-vendor probe. Evidence:
  `%TEMP%/baton-m5-approvals-final-workspace-tests.log`.
- `test --workspace`: **30 passed, zero failed**, exit 0. Evidence:
  `%TEMP%/baton-m5-approvals-final-default-tests.log`.
- Explicit `test -p baton-adapter-codex --all-features --test handshake
  installed_codex_initializes_in_isolated_home_without_model_turn -- --ignored
  --exact`: **1 passed**, exit 0, after rechecking installed Codex 0.164.0.
  Initialization only, new private home, no model thread/turn; 10.04 seconds is
  an observed test duration, not a benchmark. Evidence:
  `%TEMP%/baton-m5-approvals-final-codex-probe.log`.
- Platform-only `check -p baton-platform --all-features --features blake3/pure`
  for x86_64-unknown-linux-gnu and aarch64-apple-darwin: passed, exit 0. This is
  compile evidence, not native Unix behavior or a full foreign workspace build.
- Security/portability review: no positive execution decision, policy amendment,
  shell replay, user-config access or upload path. Synthetic scopes were torn down;
  final process inspection found no executable under this workspace's target tree.
  No service, plugin or global agent configuration was installed.

At this increment, next within M5 was initial source writer admission, remaining real-turn notification
coverage, graceful interruption and a managed synthetic-repository model task with
independent effect/native-stop evidence. Cancellation-only handling intentionally
cannot perform an approved tool operation; real source work needs a concrete scoped
policy and its own tests. No arbitrary real Codex turn is supported yet. Native
three-OS behavior, durability, privacy, performance and the real M5/M6 transfer/value
gates remain open. The next milestone is M6, after M5's exit proof; no frontend,
plugin, MCP or distributed work should bypass it. The project is not complete.

## Audited turn interruption — 25 October 2026

Plan: complete the bounded interrupt transport portion of increment 3 using the
existing Source/Session, observation journal, teardown guard and native process
scope. Bind the observed active turn before control I/O; accept supported lifecycle
interleaving without confusing acknowledgement with terminal turn or native drain.
This traces FR-03/06/12/15 and INV-04/06/07/09. Keep execution approval, source
activation and production transfer disabled.

| Test ID | Executable check |
|---|---|
| M5-I01 | interrupt_requires_an_active_turn_and_never_proves_drain: active turn, phase predecessors, host versus adapter provenance, reply after terminal turn |
| M5-I02 | interrupt_reply_and_terminal_turn_never_replace_native_drain: actual owned pipes, both reply orders, exact live retry sends no second request, unknown effect remains uncertain after stop |
| M5-I03 | interrupt_reply_rejects_ambiguous_or_nonempty_success: exact empty reply, mixed result/error including null, duplicate IDs, nonempty result, stable turn-bound request and exact method/params |
| M5-I04 | interrupt_failures_close_scope_without_claiming_a_boundary: wrong ID, vendor rejection, missing reply/timeout, no raw error content or fabricated acknowledgement |
| M5-I05 | interrupt_storage_failure_retains_only_committed_phases: failure before intent and after intent commit; no invented written/ack phase |
| M5-I06 | interrupt_wrong_turn_never_reaches_the_peer: refusal before send, gap/loss and owned child teardown |

Architecture and semantics: Source.request_interrupt consumes the live transport,
validates its actor/run binding, and records TurnInterruptPlanned before sending
the version-pinned turn/interrupt request. That HostReported intent atomically
sets STOPPING. TurnInterruptWritten is HostReported after write/flush, not receipt
proof. The capture loop matches the empty success response through durable intent
and records AdapterObserved TurnInterruptAcknowledged. Supported tool/turn events
can interleave and turn/completed may precede the reply. No reply or terminal turn
marks native drain or resolves Unknown tool effects. Explicit supervisor stop is
still required for native drain evidence; resource reconciliation remains separate.

Request IDs and phase keys bind the run/actor/epoch/lease, adapter/version, thread
and turn. An exact retry on the same live transport with written evidence returns
current metadata without sending. Intent without written evidence is ambiguous
and cannot replay. No saved record/PID can recreate a Source or process permit.
Malformed, rejected, unsupported or uncorrelated messages stop the owned scope and
record gap/loss when possible. Raw error payloads are discarded. Storage failure
can prevent the final audit; only committed phases are facts. I/O deadlines remain
positive and at most 30 seconds; synchronous transactions/cleanup are additional.

Protocol/parser tests were written first. The initial protocol run failed on the
missing interrupt variants; the native tracer then passed. A subsequent strict
parser test failed on result plus error:null; the decoder was changed to mutually
exclusive strict success/failure envelopes. No suppression was added. All focused
adapter/protocol tests then passed. Fixture peers report synthetic events and do
not execute Codex tools or call a model.

Compatibility/deviations: generated installed Codex 0.164.0 interrupt schemas and
official docs checked. Database schema 14 adds explicit interrupt journal kinds;
schemas 1–13 are preserved/refused without migration. Envelope schema remains
0.1.0. ADR-031 records these material choices. The existing generic control-key
helper is reused for approval and interruption phases. No new dependency or
dependency version, daemon, model turn, cloud upload, shell endpoint, UI, MCP or
plugin is added.

The source_cursor remains local accepted observation order, now including host
control phases; it is not a vendor-issued replay cursor or proof of complete
coverage. Portability review found no new OS-specific path or process primitive.
Security review confirmed native tool admission requires RUNNING, so STOPPING
denies fresh coordinator tool permits while late observed events remain facts.

Files added: crates/baton-protocol/tests/interrupts.rs. Files changed: protocol
adapter correlation and run/origin projection; Codex parser/client/native fixture
and tests; store schema/version checks and old-schema tests; README, current-version
report headers, architecture/protocol/security/integration/testing/roadmap/reference
and decision records. Production activation stays BLOCKED; no M5 exit is claimed.

Verification on the same Windows 10.0.26300.0 / Rust 1.98.0 GNU / Git
2.51.0.windows.1 host; no `.git` directory/revision is available. All commands use
`cargo +stable-x86_64-pc-windows-gnu` and `--locked`:

- Focused `test -p baton-adapter-codex -p baton-protocol --all-features`:
  **48 passed, zero failed**. Evidence:
  `%TEMP%/baton-m5-interrupt-focused-tests.log`.
- Final `test --workspace --all-features`: **165 passed, zero failed**, exit 0.
  Six ignored entries remain five subprocess helpers invoked by parent tests and
  the separately opt-in installed-vendor probe. Evidence:
  `%TEMP%/baton-m5-interrupt-workspace-tests.log`.
- Final `test --workspace`: **31 passed, zero failed**, exit 0. Evidence:
  `%TEMP%/baton-m5-interrupt-default-tests.log`.
- `fmt --all -- --check`, default/all-feature all-target Clippy with warnings
  denied, and all-feature/all-target build: passed. Two test-only clone lint
  findings were fixed without suppression before the final runs.
- Platform-only checks with `blake3/pure` for Linux x86_64 and macOS arm64:
  passed, exit 0. Full foreign workspace builds/native Unix runtime are unverified.
- Installed native binary still reports Codex 0.164.0. Real vendor initialization
  was already tested in the preceding increment; it was not rerun here. No real
  vendor turn or interrupt, authentication flow, user data transfer or model call
  was performed. Synthetic peer interruption is the only new runtime evidence.
- Final process inspection found no executable under the workspace target tree.
  No service/global configuration, plugin, cloud component or frontend was installed.

Next within M5: implement initial source writer admission and the missing real-turn
notification/context coverage, then drive an explicitly scoped synthetic-repository
model task and interruption with independent effect/native-stop evidence. No
arbitrary real Codex task is supported yet. Raw-output privacy, resource-specific
effect reconciliation, durable lifecycle recovery, native three-OS behavior,
durability/performance and real M5/M6/value gates remain open. Recommend proceeding
within M5; the next milestone is M6 only after M5's real source exit proof. The
complete project goal remains unfinished.

## Source admission compatibility tracer — 25 October 2026

Plan: validate the vendor control surface before adding a source writer flag or
grant. Existing native ToolPermit already admits bounded trusted-host operations
for the initial reserved task. The missing part is binding actual Codex tool
requests to those permits while preventing uncontrolled built-in write paths.
This tracer covers FR-03/06/15/16 and INV-04/06/09; it does not satisfy source
admission or the M5 exit gate.

- M5-G00: local_provider_reader_is_bounded_and_rejects_credentials checks the
  test-only HTTP reader: 16 KiB header/2 MiB body bounds, fixed Responses route,
  credential-header refusal, duplicate length, truncation and a valid request.
- M5-G01: installed_codex_advertises_only_baton_controlled_tools is an explicit
  opt-in compatibility experiment. It starts the installed native Codex under the
  existing Session/Job Object, with a fresh home and synthetic Git fixture. Model
  requests go to a temporary 127.0.0.1 listener, with no credential in its explicit
  environment/provider configuration. The local fixture returns a synthetic SSE
  completion. Only tool names enter assertion diagnostics; raw model requests,
  generated instructions and session contents are not committed or logged.

The probe uses generated --experimental schemas because dynamicTools is omitted
from the stable schema bundle and requires experimentalApi=true. Production
Source initialization stays in stable mode and no production model-turn API is
enabled. The test bypasses the production normalizer deliberately; it records an
explicit observation gap before independent native stop. It never promotes a raw
probe stream or native drain into complete source coverage. Database remains
schema 14 and envelopes remain 0.1.0; no dependency changed.

Actual installed Codex 0.164.0 results:

| Candidate profile | Advertised tool names |
|---|---|
| Shell disabled; agents/apps/web search disabled | request_user_input, view_image, get_goal, create_goal, update_goal, baton_probe |
| Also features.goals=false and tools.view_image=false | request_user_input, view_image, baton_probe |
| Also canonical features.view_image=false | request_user_input, baton_probe |

The native feature list identifies view_image and goals as stable flags. The
documented tools.view_image=false alone did not remove the image tool in this
observed profile; the canonical feature flag did. Do not infer behavior for other
versions/models or bypass inventory validation. The strict G01 assertion expects
only baton_probe and **failed in all three runs**, exit 101. Final evidence:
`%TEMP%/baton-m5-admission-final-probe.log`; earlier candidate evidence:
`%TEMP%/baton-m5-admission-probe.log` and
`%TEMP%/baton-m5-admission-constrained-probe.log`. These failures were not converted
into a pass by removing the remaining tool from diagnostics or inventing control.

This does not establish that request_user_input is a workspace writer or that a
controlled source is impossible. It identifies an additional client-mediated
interface needing explicit coverage. Production currently stops unsupported
requests. Dynamic item/tool/call request/result execution is also not implemented.
Before source admission, validate those callback paths with native request traces
and connect bounded operations to the existing fresh-only ToolPermit. Inventory
absence is necessary evidence for a restricted profile, not proof against every
unadvertised tool path, inherited policy or vendor bug. No blanket sandbox or
global network-isolation guarantee is claimed; only the configured/observed model
route was loopback. No real model/provider account was used.

Files added: tests/support/admission_probe.rs. Files changed: handshake test module
inclusion and this compatibility/architecture/security/roadmap/reference/decision
documentation. No core production code, writer permission, positive approval,
schema, CLI, frontend, plugin, MCP or cloud service was added. ADR-032 records the
bounded resolution: keep source writer admission disabled and use native evidence
to finish the missing callback controls within M5.

Verification for this test-only increment, using Windows GNU Cargo with --locked:

- `cargo +stable-x86_64-pc-windows-gnu test -p baton-adapter-codex --all-features`:
  **27 passed, zero failed**, exit 0. Its two ignored cases are the installed
  initialization probe and the explicitly failing inventory experiment. Evidence:
  `%TEMP%/baton-m5-admission-adapter-tests.log`.
- All-feature/all-target workspace Clippy with warnings denied: passed, exit 0.
  Formatting passed. Existing core/schema/dependencies were unchanged; the full
  workspace/default and native foreign checks were not repeated. The preceding
  165/31 workspace results remain historical evidence, not new results here.
- Explicit G01 with BATON_CODEX_BINARY set: **failed**, exit 101, after actual
  loopback request receipt and independent native stop. Do not include it in the
  27 passing ordinary tests. Installed native version/public schema is 0.164.0.
- Final process inspection found no workspace target executable or Codex stdio
  App Server process. No global config/service or repository source file was changed.

Next within M5 is explicit client-input/dynamic-tool callback coverage and bounded
native permit integration, using this tracer to verify actual vendor behavior.
This is independent work available without weakening the failed inventory claim.
Real model source work, verified source boundary, Claude receiver/M6 and the value
comparison remain open. Recommend continuing within M5. The full project is not
complete, and this experiment is not a reason to bypass the source safety gate.

## Negative client callbacks — 25 October 2026

Plan: close the bounded callback observation/refusal gap before enabling any
positive action. Reuse Source, the authenticated journal, deterministic server
request keys and existing teardown; keep initialization/registration and source
writer gates unchanged. This traces FR-03/06/12/14/15 and INV-04/07/08/09.

| Test ID | Concrete check |
|---|---|
| M5-D01/U01 | callback_refusal_binds_payload_and_has_distinct_phases: payload/scope binding, predecessor/origin rules, no UserInput written phase, cross-method ID collision, no written evidence after resolution |
| M5-D02/U02 | callback_requests_are_scoped_hashed_and_refused: exact generated methods, required identity/input fields, raw-text omission, byte fingerprint and fixed failure-only result |
| M5-D03 | dynamic_callback_is_refused_without_executing_or_inventing_effects: actual owned pipe, planned/written/resolved provenance, no tool effect, no stored argument text |
| M5-U03 | user_input_request_blocks_without_answering_the_user: durable input-required refusal, recovery/gap and native stop, no response or question text |
| M5-D04 | callback_payload_conflicts_and_resolved_reuse_stop_capture: changed arguments and terminal ID reuse refuse |
| M5-D05 | callback_storage_failure_retains_committed_intent_only: before/after intent commit, no fictitious written phase |
| M5-D06 | installed_codex_dynamic_refusal_reaches_local_provider: actual vendor emits callback and forwards failure to loopback fixture |
| M5-U04 | installed_codex_input_request_can_be_stopped_without_answer: actual plan-mode input callback, no answer, native stop |

Schema 15 adds CallbackRefusal with typed request ID, opaque correlation ID
(vendor itemId or callId), kind, raw-frame fingerprint and phase. Planned/Written
are HostReported; Resolved is AdapterObserved. Approval/callback phase keys share
one server-request identity domain; changing methods or payload under an ID
conflicts. Exact normalized retries remain historical metadata, never a permit.
Raw frame hashing deliberately treats changed serialization as a conflict and
does not claim semantic equivalence. No question/argument text enters the journal;
fingerprints remain private metadata, not encrypted secrets or vendor attestation.

For exact item/tool/requestUserInput, record Planned then stop with
EXECUTION_NOT_ENABLED and gap/loss where possible. The schema has no cancel answer;
never populate answers or fabricate a Written phase. For item/tool/call, commit
refusal intent, send only fixed success:false/contentItems failure, then record
Written after flush. Optional serverRequest/resolved maps through the saved plan.
It is not proof of receipt or absence of other effects. Source identity, scope,
payload changes, storage failures and unknown messages still fail closed. Only
committed phases are facts if storage prevents final audit.

Dynamic item start/completion is ClientToolCall metadata, not an executed host
operation. No new effect, arbitrary argument handler, positive tool grant or source
activation is created. Existing unknown effects stay unresolved. Native drain and
source safe-boundary proofs remain separate. Schema 1–14 stores are preserved and
refused without migration; envelope version remains 0.1.0. ADR-033 records the
material data/provenance decisions. No dependency or dependency version changed.

The initial protocol test failed on absent callback variants. Protocol/parser
tests then passed. An additional phase test failed because a Written record could
follow resolution; validation was tightened before the final regression. Managed
native synthetic peers cover normal failures, input blocking, conflicting payloads,
resolved reuse and two storage phases. These peers never execute requested tools.

The existing raw-protocol loopback tracer now also injects model function-call
events. Installed Codex 0.164.0 emitted item/tool/call and forwarded the fixed
failure in the next local provider request. In a separate plan-mode case it emitted
item/tool/requestUserInput; the test stopped the owned scope without answering.
Both explicit native probes passed. They bypass the production Source normalizer
for registration and mark their stream as a gap before native stop: compatibility
proof only, not complete production source capture or actual model reasoning.
No real model/account was used. The strict G01 Baton-only inventory experiment
remains a separate known failure and was not converted into an acceptance pass.

Files added: crates/baton-protocol/tests/callbacks.rs. Files changed: protocol
adapter kinds/validation/origins; Codex parser/client/native fixture/tests and
loopback probe; store schema/version and legacy-schema checks; README, current
report headers, architecture/protocol/security/integration/testing/roadmap/reference
and decision records. No frontend, distributed backend, MCP, plugin, shell bridge,
cloud path, positive approval or production model launcher was added.

Verification on Windows 10.0.26300.0 / Rust 1.98.0 GNU / Git
2.51.0.windows.1; no `.git` directory/revision is available. Commands use
`cargo +stable-x86_64-pc-windows-gnu` and `--locked`:

- Final `test --workspace --all-features`: **172 passed, zero failed**, exit 0.
  Nine ignored entries are five subprocess helpers invoked by parent tests,
  installed initialization, strict inventory, and the two separate callback probes.
  Evidence: `%TEMP%/baton-m5-callback-workspace-tests.log`.
- Final `test --workspace`: **32 passed, zero failed**, exit 0. Evidence:
  `%TEMP%/baton-m5-callback-default-tests.log`.
- Earlier focused adapter/protocol run: **55 passed, zero failed**. The terminal
  phase-order assertion was added afterward and passed in its explicit test and
  final workspace run. Evidence: `%TEMP%/baton-m5-callback-focused-tests.log`.
- Explicit native D06 and U04 using `test -p baton-adapter-codex --all-features
  --test handshake <exact-test-name> -- --ignored --exact`: **1/1 passed each**.
  Evidence: `%TEMP%/baton-m5-dynamic-native-probe.log` and
  `%TEMP%/baton-m5-input-native-probe.log`. Installed version was rechecked as
  Codex 0.164.0. These are local fixture responses, not a real model/account.
- Explicit G01 rerun after probe refactoring: **failed**, exit 101, with
  request_user_input and baton_probe advertised. Evidence:
  `%TEMP%/baton-m5-callback-inventory-probe.log`. This compatibility failure is
  separate from passing ordinary tests and remains unwaived.
- Formatting, default/all-feature all-target Clippy with warnings denied and
  all-feature/all-target build: passed. Platform-only Linux x86_64/macOS arm64
  checks with blake3/pure passed. Full foreign builds/native Unix runtime remain
  unverified. No new OS-specific process/path primitive was introduced.
- Security review: no positive result, answer generation, generic handler,
  execution permit reconstruction or upload path. Raw arguments/questions stay
  transient. Store read/retry evidence validation covers the new origin/kinds;
  legacy schema refusal is tested. Final inspection found no workspace target
  executable or Codex stdio App Server process remaining. No service/global
  configuration was installed.

Next within M5: bind concrete bounded operations to fresh native tool permits,
implement controlled thread/tool registration and complete production capture,
then drive actual source work and interruption with independent boundary/effect
evidence. A generic dynamic tool remains disabled; all requests currently receive
failure. Required user input stops instead of answering. Real model usefulness,
native three-OS behavior, privacy/durability/performance and full M5/M6/R0 gates
remain open. Recommend proceeding within M5; M6 is the next milestone only after
the real source exit proof. The complete project goal remains unfinished.
