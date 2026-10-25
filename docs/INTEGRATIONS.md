# Native Agent Integration Specification

**Goal:** Record reliable, provenance-rich execution evidence from Codex and Claude Code, supervise managed processes and restore another agent without relying on raw chat-summary prompts.

## 1. Agent-agnostic adapter interface

An adapter provides: `identify/runtime capabilities`, `start managed session`, `subscribe to normalized events`, `request graceful interruption`, `report termination/quiescence evidence`, `prepare receiver context`, `activate receiver`, `stop/recover`. These are conceptual responsibilities, not finalized programmatic function names.

Capabilities must distinguish **observation** (tool calls seen) from **control** (can prevent writes or terminate managed descendants). Adapter registration must report version, OS-specific limitations, event coverage and supported handshake. Default to read-only/degraded for unsupported guarantees.

## 2. Codex source adapter — preferred approach

Implemented M5 status, 25 October 2026: `baton-adapter-codex` performs a bounded
managed initialize/initialized exchange, tested against installed Codex 0.164.0
and synthetic peers. It retains the native task/run binding without starting a
thread or turn, approving a tool or claiming source coverage. Generated installed
schemas are used for compatibility discovery; incomplete event coverage, writer
admission and safe-boundary support keep real source continuation unavailable. The API is an
experimental trusted embedding, not an agent-facing shell or CLI launcher. See
M5-IMPLEMENTATION.md and ADR-028 before extending it.

The subsequent schema-12 increment captures the supported turn lifecycle and
command/file-change item boundaries, retaining IDs, reported outcome and exit
code without raw text. Correlation/dedup and evidence checks use the existing
authenticated journal. These observations cannot certify complete coverage or
settle unknown effects. Unsupported message/tool types stop capture with gap/loss;
normal real-turn coverage remains incomplete. See ADR-029.

Schema 13 adds passive start/completion metadata for userMessage, agentMessage,
reasoning, plan and contextCompaction, without storing their contents. Only command
and file-change approval methods are handled, always with cancel. A durable host
intent precedes the reply; a host-written record follows write/flush. Vendor
resolution is a separate observation correlated to the original request. None
settles tool effects or grants execution. Typed string/integer RPC IDs cannot
change scope; resolved-ID reuse fails closed. Unsupported requests or missing
cancel support stop the scope. Storage failures retain only committed evidence.
This is synthetic-peer coverage, not a real source-work gate. See ADR-030.

Schema 14 adds request_interrupt for a live, observed active turn. It journals
host intent before sending turn/interrupt and host-written evidence afterward.
capture_next accepts only a correlated empty success reply; vendor rejection or
malformed/unknown replies close capture. Supported item/turn notifications may
interleave, including turn/completed before the reply. Exact live retries with
written evidence do not send again. No stored PID/journal entry can recreate the
transport, and ambiguous intent cannot be replayed. Native stop and Unknown effect
reconciliation remain independent; this is not real-session coverage. See ADR-031.

The test-only source-admission tracer uses experimental dynamicTools under native
Codex 0.164.0 against a loopback mock provider. Stable schema generation omits that
field; --experimental includes it. Production Source has not opted into this mode.
The observed restricted profile still advertises request_user_input alongside the
registered Baton function. The strict Baton-only probe fails; do not treat that
inventory as a writer capability or assume the input callback is a filesystem
writer. Schema 15 now supplies bounded refusal handling, while positive native
permit integration remains required. See ADR-032/033 and the M5 report.

The generated experimental schema uses item/tool/requestUserInput. No cancel
answer is defined, so the adapter records an input-required refusal and stops
without inventing user intent. item/tool/call receives a fixed success:false
result after durable intent, then Written evidence after flush. Optional
serverRequest/resolved is correlated separately. Dynamic item start/completion
is metadata, not host execution. Native loopback probes verified failure-result
forwarding and stop-on-input using raw test protocol; production end-to-end source
capture/registration remains unproven. No real model or positive admission ran.

**Primary official surface:** Codex App Server, which exposes structured thread/turn lifecycle, item events and interruption. Run app-server under Baton supervision and bind each task to its thread ID, turn ID, agent version, working directory and epoch. Support approvals without silently auto-approving operations.

Implementation requirements:

- Record each relevant observed lifecycle/item event with event sequence, vendor IDs, normalized class, and raw evidence reference.
- Maintain replay/dedup cursor across daemon restart; do not invent events after disconnect or mistake UI notifications for durable tool completion.
- Capture command starts/completions, content changes, errors, approval requests, turn status; do not depend on private, undocumented transcript internals.
- For interruption request, invoke supported native operation and wait for terminal turn status. Independently prove local supervisor quiescence and effect-ledger safety.
- Never promote interrupted-turn status alone to VERIFIED, especially after background process spawn.
- If user runs Codex outside Baton's supervision, allow optional forensic import but do not assert equivalent write fencing.

**Compatibility:** App Server protocol can evolve. Feature-detect notifications; pin supported versions in tests; document schema changes. Fallback to bounded `codex exec` for simple test harnesses only; it does not replace rich app-server integration for production.

## 3. Claude Code receiver and reverse source adapter

**Primary surfaces:** Claude Code Agent SDK/CLI for managed lifecycle, structured messages and permissions; hooks for lifecycle/tool observation and supplementary mutation guards. Package hooks in optional Claude Code plugin where supported.

- Start receiver only **after** coordinator commits new epoch and passes that authority to the managed session.
- Supply task objective, integrity/restore report, concise provenance-linked continuation packet, and read-only evidence tools; keep vendor's own prompt/security boundaries intact.
- Preserve native existing session resume where safe/available, but do not confuse Claude native session resume with portable cross-provider continuation.
- Observe `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, stop/session lifecycle events when supported; guard operations against stale ownership. Hook behavior, timeouts and bypasses must be tested on the exact installed Claude version.
- **Critical:** Claude Code `--bare` skips normal project plugins/hooks/MCP and may use a distinct authentication path. Do not select it for the hooked production receiver unless explicit alternate instrumentation and auth are implemented.
- Account for MCP tool execution and delegated agents/child processes; not every code mutation is a visible file-edit tool.
- For reverse Claude -> Codex, apply same checkpoint/effect/lease protocol; no provider gets a special lower standard.

## 4. Event normalization and provenance

Each canonical event includes task, actor, adapter/version, session/turn/vendor operation ID, monotonic task sequence, time, parent/correlation ID, scope, action kind, outcome state and evidence blob hash. Store vendor raw data separately behind redaction and permissions. Support interrupted/incomplete tool outcomes as first-class events.

Recommended event kinds: session opened/closed, goal asserted, tool started/completed/failed/uncertain, command started/completed, file changed, tests executed/verified, checkpoint stage/publish, lease changed, transfer prepared/committed/blocked, explicit decision or claim, environment change, unresolved effect.

**Fact** = verified observable event such as a completed command exit code with persisted output. **Claim** = source agent or user says an approach is correct or next step needed. **Inference** = Baton-generated interpretation. The receiving agent must be told which class it is reading.

## 5. Continuation packet

Mandatory: original task goal, current task/transfer/epoch, checkpoint root and changed files, last complete tool events, verified failures/test results, unresolved effects, environment gaps, explicit constraints, pending-action suggestion labeled appropriately and evidence tool access. Do not pass entire raw transcript by default.

Context compiler strategy: deterministic priority ranking by goal relevance, active failures, last events, changed files and explicit decisions. Enforce token/size cap; safety warnings exempt from context trimming. Use evidence IDs and on-demand read methods. No embeddings required at first. Never elevate text sourced from repo/transcripts to higher trust than system/developer/user instructions.

## 6. MCP tool surface (proposed)

MCP makes Baton available *inside* Codex, Claude and compatible hosts. It is not sufficient for lifecycle capture or exclusive write control.

**Read tools:** `baton_status`, `baton_tasks`, `baton_checkpoint_details`, `baton_execution_events`, `baton_changed_files`, `baton_test_failures`, `baton_decisions`, `baton_recovery_report`.

**Controlled mutation tools:** `baton_request_transfer`, `baton_create_checkpoint`, `baton_accept_transfer`, `baton_report_progress`. Each action requires coordinator-issued task capability, current epoch and idempotency. Do not expose a generic `execute_shell`, direct SQL update, arbitrary path reader or force-lease tool.

Default policy: read-only tools available after repo trust and opt-in; mutations require explicit owner identity and approval appropriate to the operation. Tool descriptions must clearly say when a call is advisory vs authoritative. If daemon not running, report a precise recoverable error rather than silently launching with different permissions.

## 7. Plugin/skill roles

**Codex integration:** root portable `plugin.json` and `mcp.json` plus `skills/setup/SKILL.md` and task/transfer skills are an installable wrapper. Hook-bearing Codex desktop plugins currently have distribution restrictions; native installer and per-host configuration remain the dependable route. The plugin must never claim to grant enforcement absent supervisor attachment.

**Claude Code integration:** `.claude-plugin/plugin.json`, packaged `skills/`, optional `hooks/`, `.mcp.json`; publish through a marketplace once validated. Install and trust must remain transparent; a plugin can enhance an already installed daemon but should not assume the daemon or project trust exists.

**Skills** teach when to request `checkpoint` or `transfer` and how to inspect status. **MCP tools** provide controlled operations. **Hooks** provide observation/gates only where supported. **Native daemon** owns the durability and authority.

## 8. Compatibility test matrix

Every release should test version(s) of Codex App Server, Claude Code CLI/SDK, hooks, stdio MCP, Windows/macOS/Linux, and no-agent/agent-crash modes. Include plugin installed vs disabled, hook timed out, MCP missing, bare/headless mode, user refuses approval, detached background process and interrupted deployment. Document exact tested versions in generated release artifacts. Do not infer support from only a vendor documentation page.

## 9. Vendor references

Use `REFERENCES.md` for official docs. Before implementation re-check version-sensitive names and semantics; this spec describes intended behavior, not a guarantee that each vendor exposes every control in every product build.
