# Baton — Official Sources and Compatibility Checklist

**Checked:** 22 October 2026. Vendor documentation is version-sensitive. Links are source material; `PRD.md`, `docs/PROTOCOL.md`, and `docs/SECURITY.md` define Baton's proposed behavior, not vendor guarantees.

## Codex / OpenAI

1. **Codex App Server:** https://developers.openai.com/codex/app-server/ — documented protocol for thread/turn lifecycle, streamed events, approvals and interruption. Reconfirm exact event names, lifecycle result states and interrupt acknowledgement with the installed build.
2. **Codex as a platform:** https://developers.openai.com/blog/codex-as-a-platform — guidance on choosing CLI, SDK or App Server based on desired integration control.
3. **Package a plugin:** https://developers.openai.com/plugins/build/plugins — portable root `plugin.json`, root `mcp.json`, skills, optional hooks; Codex desktop manual-hook/plugin trust and directory distribution restrictions.
4. **Plugin architecture:** https://developers.openai.com/plugins/concepts/plugins — what plugins contain and which components require local availability.
5. **Codex MCP configuration example:** https://developers.openai.com/learn/docs-mcp — Codex MCP server registration and supported client usage.

## Claude Code / Anthropic

6. **Plugins overview:** https://code.claude.com/docs/en/plugins — `.claude-plugin/plugin.json` and bundled skills, agents, hooks and MCP configurations.
7. **Hooks reference:** https://code.claude.com/docs/en/hooks — `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, timeouts, blocking behavior and trust configuration.
8. **MCP guide:** https://code.claude.com/docs/en/mcp — local/remote server registration and scopes.
9. **Claude Agent SDK:** https://platform.claude.com/docs/en/agent-sdk/overview — inspect exact supported streaming, permissions, sessions and process-control APIs before relying on them.
10. **Claude Code plugins install/management:** https://code.claude.com/docs/en/plugin-marketplaces — verify marketplace and local install commands against active docs/CLI; URLs and syntax may evolve.

## Git, SQLite and MCP

11. **Git worktree:** https://git-scm.com/docs/git-worktree — linked worktree semantics and lifecycle. Worktrees are workspace isolation, not OS privilege isolation.
12. **SQLite WAL:** https://www.sqlite.org/wal.html — writer/read concurrency and checkpoint behavior; do not infer fully atomic multi-store commits.
13. **MCP transport specification:** https://modelcontextprotocol.io/specification/2025-06-18/basic/transports — stdio and streamable HTTP mechanics; verify current revision during implementation.
14. **MCP security guidance:** https://modelcontextprotocol.io/specification/2025-06-18/basic/security_best_practices — apply transport and tool security alongside native daemon authorization.
15. **BLAKE3:** https://github.com/BLAKE3-team/BLAKE3 — content hashing implementations and benchmark reference.
16. **Zstandard:** https://facebook.github.io/zstd/ — compression tradeoffs and implementations.

## Minimum revalidation before implementing adapters

- Can Codex reliably report completed vs interrupted turns, tool boundaries and event correlation? Verify with actual App Server traces.
- What child processes can survive an interrupt on Windows/macOS/Linux? Test, don't infer from turn status.
- Does Claude hook denial work on exact named tool types, and what happens on hook timeout/failure? Test fail-closed configurations.
- Does `--bare` or other headless mode alter Claude plugin/hook/MCP behavior? Verify under the current binary.
- Which Codex plugin distribution route permits bundled hooks? Do not promise one-click public-directory safety enforcement without confirming current policy.
- Can local MCP stdio access the required daemon socket securely under each OS? Test user credentials and permission boundaries.
- Are provider session IDs stable across restarts and upgrades, and what recovery paths are supported? Treat unknown or missing information as reduced capabilities.

## Evidence hierarchy

Use (1) official current protocol docs and tested public APIs; then (2) observed behavior under pinned installed versions; then (3) reputable open-source implementations for examples. Do not elevate screenshots, social posts, undocumented/private caches or LLM-generated claims into guaranteed interface contracts.

## M1 storage compatibility check — 22 October 2026

Validated [SQLite WAL guidance](https://www.sqlite.org/wal.html),
[synchronous pragma](https://www.sqlite.org/pragma.html#pragma_synchronous), and
[rusqlite 0.40.2 connection APIs](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Connection.html).
WAL is single-host and must not be used as a cross-machine/network filesystem
coordinator. The WAL-reset bug is fixed in SQLite 3.51.3 and later; the M1 engine
rejects older SQLite builds and uses bundled SQLite 3.53.2 in this lockfile.
Per-connection WAL, FULL and foreign-key settings are checked; there is no
fallback to a weaker journal mode. Filesystem/hardware durability still depends
on correct local fsync/locking behavior. Vendor agent APIs were not used in M1.

## M2 Git/codec compatibility check — 22 October 2026

Checked official [worktree](https://git-scm.com/docs/git-worktree),
[update-index](https://git-scm.com/docs/git-update-index), and
[diff](https://git-scm.com/docs/git-diff) documentation. Tested Git for Windows
2.51.0 with --no-checkout, NUL-delimited index plumbing, and no-filter hashing.
The two intent-to-add visibility flags are documented as experimental; unsupported
flags fail capture rather than being ignored. Sparse/split/flagged/conflicted
states are refused; partial-clone lazy fetching is disabled/refused.

Checked the [zstd decoder API](https://docs.rs/zstd/0.14.3/zstd/stream/read/struct.Decoder.html)
for window/output bounds. The lockfile uses zstd 0.14.3 / Zstandard 1.5.7.
For Windows hardlinks, stable Rust's
[by-handle MetadataExt fields](https://doc.rust-lang.org/std/os/windows/fs/trait.MetadataExt.html)
are still experimental; use the safe
[winapi-util Information API](https://docs.rs/winapi-util/0.1.11/winapi_util/file/struct.Information.html)
instead of nightly Rust or unsafe code in the workspace. This does not establish
directory flush/ACL guarantees. No vendor agent interface was used in M2.

## M3 process-control compatibility check — 22 October 2026

Reviewed Microsoft's [Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
guidance and pinned source for [process-wrap 10.0.1](https://docs.rs/process-wrap/10.0.1/process_wrap/)
and [win32job 2.0.3](https://docs.rs/win32job/2.0.3/win32job/struct.Job.html).
Process-wrap supplies suspended assignment/resume and termination, but wrapper
wait paths are not a sufficient emptiness oracle. Baton separately queries the
owned job's process list. win32job uses a bounded list; query errors fail closed.
Nested-job launch, root-exit/child survival and kill-on-close teardown were tested
on the local Windows GNU host. Unix process groups cannot prevent descendants
leaving the group; they remain a cooperative control with Unknown drain status.
No Codex/Claude interface was used or compatibility claimed in M3.

## M4 local-boundary compatibility check — 22 October 2026

Reviewed [windows-permissions 0.2.4](https://docs.rs/windows-permissions/0.2.4/windows_permissions/wrappers/fn.SetNamedSecurityInfo.html)
and the pinned source for [interprocess 2.4.6](https://docs.rs/interprocess/2.4.6/interprocess/).
Use direct safe permission wrappers with explicit object types and DACL flags;
the WindowsSecure multi-field helper is not used. The local pipe listener defaults
to rejecting remote clients. Client handles explicitly request READ_CONTROL and
overlapped I/O, with identification-only SQOS and an owner check before sending.

The [Microsoft named-pipe client contract](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-client)
documents ERROR_PIPE_BUSY when all instances are in use. The CLI waits, bounded
to three seconds, before sending any request. This is connection setup, not replay
of an uncertain operation. Actual concurrent-client tests reproduced the busy
condition and passed with the bounded wait. Tokio 1.54.1 and Clap 4.6.7 are pinned
by the lockfile and exercised in native CLI/daemon integration tests. No vendor
agent compatibility is claimed by this local-control work.

## M5 discovery — 23 October 2026

Installed Codex reports 0.163.0. Its local App Server help and JSON-schema
generator were executed successfully without a model turn. Re-fetched the
[official App Server page](https://developers.openai.com/codex/app-server), which
now links to [ChatGPT Learn's App Server documentation](https://learn.chatgpt.com/docs/app-server).
Use generated version-specific schemas and runtime feature checks; documentation
is not evidence of complete event coverage or safe native quiescence. No
real-session support is certified. See M5-IMPLEMENTATION.md for the plan/limits.

## M5 handshake compatibility — 25 October 2026

The installed binary now reports Codex 0.164.0. Regenerated its public JSON schemas
and checked required initialization fields and explicitGatewayOauth capability.
Re-fetched the [official App Server page](https://learn.chatgpt.com/docs/app-server)
for JSONL and handshake order. The actual managed native test completed the local
handshake and stop with a new private home, without a thread or model turn.
This certifies only that observed Windows handshake, not source-work/event safety.

Checked pinned local source for Tokio 1.54.1 ChildStdin/ChildStdout::from_std and
process-wrap 10.0.1 ChildWrapper pipe accessors. Attempts to fetch those exact
docs.rs API pages failed in the web tool; do not count them as fetched evidence.
No process-wrap upgrade or new third-party dependency was introduced. The existing
Tokio dependency now enables process I/O. Native Unix runtime remains unverified.

The schema-12 lifecycle increment additionally inspected the generated 0.164.0
TurnStarted/TurnCompleted and ItemStarted/ItemCompleted notification schemas:
item boundaries carry thread/turn/item IDs and required start/completion timestamps;
command and patch status enums distinguish inProgress/completed/failed/declined.
Only this bounded subset is normalized. Synthetic native peers test it; no real
Codex turn or complete vendor-event compatibility is certified by those tests.

## M5 cancellation/passive compatibility — 25 October 2026

Inspected generated Codex 0.164.0 command/file approval request/response schemas,
RequestId, ThreadItem and ServerRequestResolvedNotification. The two approval
requests require threadId, turnId, itemId and startedAtMs. RequestId permits a
string or signed 64-bit integer; Baton intentionally accepts only its bounded
adapter-ID string subset. Cancel denies the operation and interrupts the turn;
decline permits continued agent work, so Baton emits cancel only. The resolution
notification carries threadId/requestId without turnId. Bind it through the saved
request, not a guessed current turn. Passive metadata captures no text/content.

The [official App Server overview](https://learn.chatgpt.com/docs/app-server)
also describes optional availableDecisions, absent from these generated approval
parameter schemas. If supplied, require cancel among the bounded decisions. Do
not substitute accept or policy amendments. Generated interfaces and synthetic
peer tests establish this narrow parser contract; no real model approval, safe
boundary, external-effect settlement or complete source coverage is certified.

## M5 interruption compatibility — 25 October 2026

Re-read the [official App Server interruption interface](https://learn.chatgpt.com/docs/app-server)
and generated Codex 0.164.0 v2/TurnInterruptParams and TurnInterruptResponse:
request parameters require threadId/turnId and successful result is an empty
object. A separate turn/completed notification reports interrupted status. Baton
requires an exact empty success object, rejects mixed result/error envelopes,
and correlates the ID through the durable run-bound plan. Synthetic peers test
reply-before-terminal and terminal-before-reply; these orders are defensive test
coverage, not assertions about a guaranteed vendor ordering. Neither message
establishes native process drain or settled external effects. No real model turn
or interrupted vendor work was started for these interface checks.

## M5 source-admission tracer — 25 October 2026

Re-read the [App Server dynamic-tool interface](https://learn.chatgpt.com/docs/app-server),
[configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)
and [custom provider configuration](https://learn.chatgpt.com/docs/config-file/config-advanced).
Generated Codex 0.164.0 schemas with --experimental: dynamicTools requires
experimentalApi=true and uses typed function/namespace specs. The stable bundle
omits this field. This does not authorize switching production initialization to
experimental mode or granting native writes.

The native loopback tracer used a custom Responses provider without credentials,
fresh home and synthetic repository. In the observed request, disabling shell
removed command/patch tools; disabling goals removed goal tools. Documented
tools.view_image=false alone did not suppress view_image in this installed profile;
the native feature list's canonical stable features.view_image=false did. The final
inventory remained request_user_input plus baton_probe, so the strict Baton-only
experiment failed. See M5-IMPLEMENTATION.md for exact profiles, commands and results.
Do not generalize this observation to other versions/models or infer comprehensive
sandbox/network enforcement from the advertised tool list. No real model was used.

## M5 negative callback compatibility — 25 October 2026

Inspected generated experimental 0.164.0 ToolRequestUserInputParams/Response,
DynamicToolCallParams/Response and dynamic item status definitions, and re-read
the [official callback interface](https://learn.chatgpt.com/docs/app-server).
The generated ServerRequest method is item/tool/requestUserInput; parts of the
overview shorten that spelling. Baton accepts the generated method rather than
guessing aliases. Input params require isBlocking, itemId, questions and thread/
turn IDs. Answers map question IDs to answer arrays; no cancellation enum is
defined. The adapter therefore stops without fabricating a response.

Dynamic callback params require arguments, callId, tool and thread/turn IDs.
The failure response has success:false and contentItems with inputText content.
Dynamic lifecycle statuses are inProgress/completed/failed. Native Codex against
the local mock Responses provider emitted both expected callbacks. A fixed dynamic
failure reached the fixture in the next request; plan-mode input was observed and
the owned scope stopped without an answer. These are raw-protocol compatibility
tests, not a real model, complete production Source capture or source writer proof.
Production initialization remains stable; experimental registration is test-only.
