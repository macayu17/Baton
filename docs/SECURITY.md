# Security, Privacy and Threat Model

**Principle:** Baton handles private code, agent transcripts, filesystem state and potentially powerful process-control operations. It is a privileged local developer tool, not merely a note-taking plugin.

## 1. Security objectives

- Prevent unauthorized task takeover in enforced coordinator paths.
- Prevent accidental replay of high-impact external operations.
- Never silently export repositories, secrets or transcripts.
- Keep agent-generated context from escalating its instruction authority.
- Preserve developer checkout integrity and enable safe rollback/recovery.
- Make capability limitations explicit rather than pretending hooks/leases guarantee sandbox isolation.

## 2. Threat actors and sources

- Malicious or compromised source repositories, prompts, README files and shell outputs (prompt injection).
- A stale or disconnected agent continuing to write after nominal lease expiration.
- A teammate with access to one task trying to query another's private artifacts.
- A malicious/untrusted MCP client making forged task mutation requests.
- Compromised plugin scripts or supply-chain binary downloads.
- Accidental capture of tokens, SSH keys, `.env`, database dumps and private code.
- Hostile symlinks/junctions/path traversal, filename case collisions and crafted Git submodules.
- A local same-user process abusing IPC; isolate permissions but be explicit that same-user malware may be outside threat guarantees.

## 3. Trust boundaries

The implemented M4 local boundary uses an exclusive owner-checked state lock and
an OS-derived principal. State directories/files with uncertain ownership or
broad access refuse without automatic ACL repair. Windows creates a protected
owner ACL before storing data; Unix uses private modes. State leaf symlinks,
reparse points, capability-file hardlinks and Windows UNC paths are refused.
Root-owned ancestor aliases in non-writable system directories may resolve the
approved Unix state path (including macOS /var); the state leaf cannot be a link.

The opt-in daemon exposes only owner-scoped named pipes or a private Unix socket,
with no TCP bind. Windows pipe creation rejects remote clients; the client checks
pipe ownership and uses identification-only SQOS, preventing impersonation.
Unix checks peer effective UID on both ends. The endpoint locator is not a
credential or liveness proof. CLI requests cannot provide actor identity or
supervisor observations. Local operation fingerprints have a separate trust
domain from native embedding operations. See ADR-020 for the implemented limits.

1. User explicitly grants repo/task scope; OS account, not an arbitrary LLM prompt, provides local authority.
2. Agent adapter identity must bind to a managed supervisor instance and recorded source session. Adapter self-reports cannot grant epochs.
3. Local IPC uses user-scoped permissions; secrets/capability tokens never logged. Loopback network is off by default.
4. MCP write tools require scoped, current lease and user policy; MCP read tools redact and enforce repo/task permissions.
5. External effect execution and privileged operations require user-approved policy, often separate from Baton authority.
6. Team sync requires authenticated users/device identities, TLS, per-task authorization and explicit consent on uploads. Encrypt sensitive blobs at rest; minimize metadata exposure.

## 4. Safe capture and data classification

Capture policy should classify data: public project state, sensitive project files, potential credentials, transcript/command outputs, environment secrets, user-provided annotations and generated inference. Default to exclude/redact known secret patterns and common sensitive paths. **Redaction is best-effort, not perfect**; consent and local-only defaults remain necessary. Avoid storing full environment variables or live API tokens. Separate small safe metadata from encrypted sensitive raw evidence.

For checkpoint correctness, a secret excluded from the snapshot must appear as an explicit **environment gap** that can block VERIFIED. Do not leak its value to Claude; provide an instruction to re-provision from approved vault/environment.

Never upload any repository snapshot, prompt, agent transcript, or command output by default. Team sync uses per-task opt-in and a clear preview of artifacts/categories shared. Support configurable retention and legal deletion limitations for backups/team servers.

## 5. File safety and isolation

The experimental Session create-only host operation pins its cwd before spawn,
requires a fresh native ToolPermit and accepts only flat portable filenames and
at most 16 KiB. A domain-separated effect ID binds the complete run/operation and
file recipe. Directory-relative create_new prevents existing leaf links or files
from being overwritten. Ownership/root identity are checked before/after access;
Windows held roots deny deletion/rename. File sync/readback establishes observed
bytes, not directory power-loss durability or future integrity. Partial failures
preserve files and attempt native teardown/loss journaling. Exact retries return
no permit, and changed recipes conflict. Positive callbacks require the separate
explicit host policy described below.
Same-user adversarial races and native Unix behavior remain release limitations;
this primitive is not a sandbox. See ADR-034 and M5-W01–06.

Use fresh worktrees and a strict approved repo root; reject path traversal and any symlink/junction resolution outside allowed roots, unless explicit policy handles it. Protect `.git` internals and don't follow untrusted links when capturing. Detect submodule and LFS dependencies and symbolic file metadata issues. Never `git clean -fdx` or destructive reset a developer checkout. Restore only Baton-owned staging locations, after path validation.

One valid ownership epoch does not imply ability to stop unknown processes. Enforced write isolation ideally uses OS/container boundaries; a file-edit hook is an additional check, not a comprehensive sandbox. For external production resources, Baton must not claim exclusive write control unless the resource explicitly supports fencing or access isolation.

## 6. Safe event replay and external effects

Schema-17 thread registration is explicit trusted embedding, not source activation.
The host pipe write has fresh native admission; separate vendor startup remains
Unknown/UNCERTAIN even on a successful acknowledgement. A generic vendor thread
can initialize configured services, so neither read-only policy metadata nor
an empty history proves every startup effect settled. Existing unresolved-effect
checks therefore deny subsequent host writes. Missing replies/metadata commits
retain uncertainty; there is no automatic RPC replay or process adoption. Default
initialization stays stable. Registration and announcement have distinct hashed
evidence, and retained thread fields must match the journal. Startup remote IDs,
server names and previews are omitted. Only reported disabled remote status is
supported; it grants no isolation proof. Other statuses, warnings and unknown
notifications stop capture. See ADR-036 and M5-R01–05.

M5-SP01 provides native evidence for this distinction: installed Codex 0.164.0
started a configured MCP child that created a temporary marker outside the test
worktree while thread/start reported readOnly, before any model turn. Disabling
that server suppressed the marker in the comparison. config/read exposed its
enabled state before registration; it is not a configuration freeze or native
confinement proof. A controlled home or a restricted tool inventory alone cannot
settle startup uncertainty. The probe changes no production authority.

Schema 19 now inspects config/read before file-thread registration and rejects
nonempty reported MCP, hook, plugin, marketplace or notify configuration, including
disabled entries. Malformed or uncorrelated replies fail closed. The fingerprint
and summary must commit before thread/start; config values remain transient.
Omitted/null entries are only absence of reported configuration. This does not
freeze layers, attest managed hooks, cover process launch before inspection or
resolve unknown startup. No configuration is rewritten. See ADR-038/M5-SP02–05.

Schema 16 provides explicit Source::with_file_creation opt-in for the single
baton_create_file_v1 recipe. Unknown tools/namespaces, extra arguments, unsafe names
and oversized content fail closed. Callback intent commits before native host
admission; host completion commits before a success response containing only the
file digest. Written means flush, and vendor Resolved remains separate. Refusal,
execution and approvals cannot rebind an RPC ID. Duplicate requests stop capture
without replay, including after external deletion of the created file. Missing
completion leaves uncertainty; missing Written after a committed completion does
not erase that completion or invent receipt. Errors preserve files and attempt
scope teardown/gap/loss. Raw contents are transient. This does not register vendor
tools, activate a source or prove isolation/event completeness. See ADR-035.

The experimental M5 schema-13 approval handler only emits cancel for recognized
command/file-change requests. There is no accept, session-wide grant or policy
amendment path. It validates bounded typed RPC IDs and thread/turn/item scope,
commits cancellation intent before pipe I/O, and distinguishes a written reply
from vendor-reported resolution. Neither proves that an external effect did not
occur. Unknown effects stay uncertain. Unsupported methods, missing cancel support
and resolved-ID reuse close the owned scope without an execution grant. Storage
failure may prevent a final audit; only committed phases can be asserted. Passive
item lifecycle metadata omits message/reasoning content, not merely log redaction.
This subset is not complete real-session coverage. See ADR-030.

M5 schema-14 interruption only sends turn/interrupt to the owned initialized
transport, bound to an observed active turn. Host intent must commit before the
send. Only its correlated empty reply can be an acknowledgement; mixed result/error
envelopes are rejected even when error is null. Vendor error payloads are discarded.
Live retries with written evidence cannot send twice; an ambiguous intent cannot
reconstruct a request. Failed/abandoned control terminates the owned scope and
records gap/loss when possible. STOPPING, acknowledgement and turn completion grant
no writer authority and do not resolve Unknown effects. Independent native drain
and resource evidence are still required; no upload or model execution API is
added. See ADR-031.

The M5 source-admission inventory probe is an explicitly invoked test, not a
production model launcher or activation gate. It uses a synthetic Git repository,
new private vendor home, credential-free explicit environment and custom provider
pointing to a temporary 127.0.0.1 listener. Its HTTP reader rejects credentials,
alternate routes/encodings, duplicate lengths and oversized/truncated requests.
Only tool names enter diagnostics; raw request/session data stays temporary. An
explicit gap marks the probe stream before native stop. Advertised inventory is
not complete sandbox/write-control proof, and no kernel-wide egress isolation is
claimed. Negative callback handling is covered by schema 15 below; the positive
dynamic execution bridge is not validated and source writer admission stays
disabled. See ADR-032/033.

Schema-15 callback refusals store identity and a raw-frame fingerprint, never
question/argument text or a fabricated answer. The exact input-request method
records a refusal plan and closes its scope. Dynamic requests receive only a
fixed failure after intent commits, with written evidence separate from vendor
resolution. Cross-method RPC-ID reuse and payload changes conflict; terminal IDs
cannot cause another response. Fingerprints are private metadata, not encryption.
No generic shell/argument execution, positive tool permission or source activation
is added. Storage failure may prevent final audit; only committed phases are facts.

The experimental M5 handshake uses only explicitly selected native stdio and
environment, both bound to the launch fingerprint; inherited environment remains
cleared. The installed-vendor test uses a new private home with no supplied user
credentials, analytics disabled and explicit gateway OAuth. These settings are
not network isolation or a filesystem sandbox. Neither the protocol client nor
its test sends a model prompt or user repository contents. Raw home, user-agent
and error payloads are bounded and discarded rather than added to diagnostics.
Unexpected handshake messages fail closed. Connect failure/cancellation terminates
the owned scope and attempts to record loss; cancellation cannot promise a journal
commit if storage fails. After success the host must stop explicitly and reconcile
uncertain drop/crash. A successful initialize response grants no writer authority,
event completeness or safety guarantee. See ADR-028.

Launch-v2 additionally binds the main executable's streamed BLAKE3 digest and
rechecks file identity/content before spawn. Windows holds a read-sharing-only
file handle through native scope ownership, preventing ordinary writes and direct
replacement until the object drops. Native permissions remain unchanged. This
does not bind dependencies, scripts or startup configuration, and path-based
exec retains race limits; Unix gains observed-change detection, not enforced file
immutability. Do not promote the fingerprint to vendor capability or resource
settlement. The Unknown startup blocker remains. See ADR-039/M5-B01–04.

M5-NC01–04 evaluate a signed Microsoft MXC executor only as an opt-in test input.
The tracer accepts base-container support with no DACL augmentation, no BFS and
no warnings/errors, then runs a benign Rust workload against temporary files and
a live loopback listener. Native denials have an uncontained access control.
Environment is explicit and LOCALAPPDATA is private to the fixture. No host-prep
tool, global exemption, user worktree ACL change or model request is made. No
SDK binary is bundled. This does not establish crash-safe resource cleanup,
immutable production policy binding or source capability. See ADR-040.

Experimental M3 launches are trusted embedding operations, not shell/MCP tools.
Their inputs and environment are not persisted as replay recipes. Run intent is
durable before native launch; recovery cannot reconstruct a launch permit from a
PID or stored record. Windows kill-on-close jobs cover the tested descendant
scope, not service/WMI/remote work initiated outside the job or arbitrary
filesystem access. Unix process groups cannot certify complete containment.
Low-level Store observations must remain unavailable to agent-facing clients.
The native Session accepts only adapter observation kinds from adapter callers.
External reconciliation by a caller-provided hash is disabled; no resource
reconciler has yet established that the external operation is settled.

Record effect classification: known read-only, reversible local, idempotent with token, externally verifiable, non-idempotent, unknown. Default unknown on crash or missing completion event. Do not replay POST/payment/deployment/migration commands simply to 'recreate context'. Query external state through a provider-specific reconciler, with explicit user approval for potentially mutating retries.

## 7. Authentication and authorization (R2)

Native scope/drain seals cannot be supplied as JSON proof flags. A live platform
object, original launch permission and exact launch fingerprint bind a source
scope; an opaque borrowed empty-scope proof binds its drain. Native operations
use a separate request domain and preserve existing task/actor/epoch checks.
Raw trusted synthetic observations remain metadata and cannot mint a native seal.
Read records are historical evidence, not process controls or writer grants.
Missing terminal execution evidence blocks use. Windows job membership does not
cover services, remote effects, unrelated writers or all shared resources; Unix
process groups cannot produce an empty-scope proof. See ADR-023.

M4 transfer intent is a metadata hold, not a source-process fence. Preflight
checks immutable checkpoint contents but cannot establish source isolation,
event coverage, vendor compatibility or effect settlement from stored assertions.
Missing proofs remain BLOCKED. Transfer records are checked against their
append-only journal and task-event evidence; corruption refuses launch/cancel.
Abort releases only transfer intent and never grants another writer or replays
a process. Metadata commit is guarded by engine checks; local process activation
and client proof-input endpoints remain unavailable.

Use per-user and per-device keys/tokens; separate roles such as owner, collaborator, reader and administrator. The server is authoritative for ownership epochs and checks task permissions in every mutation. Token revocation and expiry must be enforced. Restore clients verify downloaded artifact hashes and signatures/MACs as designed; server must reject cross-tenant references and replayed grants.

A remote network partition must not trigger blind failover: if previous process/resource access cannot be fenced, write continuation remains BLOCKED. Allow read-only inspection when safe.

## 8. Supply chain

Signed build artifacts, dependency auditing, release provenance, pinned lockfiles, minimal default privileges, no arbitrary postinstall scripts, transparent plugin hook trust prompts. Publish change logs and tested agent/runtime versions. Respect upstream terms and rate limits; use documented interfaces rather than scraping secret internals.

## 9. Auditing and incident handling

Schema 18 protects the startup gate from inconsistent mutable execution summaries.
Every execution event carries the resulting operational RunRecord digest and is
hash-bound to its task-journal entry. Reads check all execution bindings/sequences
and the latest state digest, including effect classification/status, cursors and
thread fields. Observed-run retry results must match their own recorded state;
newer legitimate events remain compatible with historical retries. Empty journals
cannot invent running processes. Mismatch preserves state and returns CORRUPT_STORE
before admission. Recomputing only the summary/event digest cannot bypass the
separate task-journal binding. This is not a MAC/signature or protection against
rewriting the whole private evidence graph. Unknown resource effects remain
unsettled; no replay or new writer grant follows. See ADR-037 and M5-SG01–03.

M5 lifecycle metadata never executes a captured command. The bounded parser stores
only supported IDs, kinds and reported status/exit code, treating the vendor as an
observation source rather than an authority issuer. Task/run/epoch/lease and actor
checks remain in the store. Normalized event-phase IDs deduplicate facts; they do
not attest raw message equality or authorize replay. Raw output/text is omitted
and unavailable, not silently labeled redacted-and-complete. Unsupported messages,
wrong binding, conflicting retries and capture loss refuse continuation. New
adapter events bind to task-event hashes checked on reads and cached replies.
Corruption preserves evidence and blocks; final loss logging remains best-effort
if the store itself cannot commit. All current vendor effects are Unknown and
cannot settle by reported success alone. See ADR-029.

M4 staging and commit consume engine-owned checks, not request proof booleans.
Production keeps fencing/isolation, adapter/event and effect guarantees missing;
PREPARED cannot be reached from native drain alone. The closed-boundary verifier
in unit tests is `cfg(test)` only and has no runtime enable switch. Even the
fixture's committed metadata cannot produce a normal launch permit. The trusted
receiver-launch API requires fresh engine checks, exact restored cwd and current
receiver authority; its launch descriptor is not accepted over IPC. It records
durable ACTIVATING before issuing one nonserializable permit. A native spawn does
not establish vendor ACTIVE. CLI activation remains EXECUTION_NOT_ENABLED, and
the production checker cannot issue a receiver permit. Stage IDs reserve their complete authenticated
request/domain and cannot be reused by another action. Commit rechecks source and
receiver bytes and lease expiry. Reads reject a task owner inconsistent with its
committed grant. Recovery preserves epoch/owner and never resumes the old source.

Receiver readiness requires the trusted host's normalized session acknowledgement
and an opaque sampled-running-root proof from the original scope. Serialized
acknowledgements, unrelated live processes and stored PIDs cannot invoke this path.
The session ID, full run binding and current lease are checked; earlier tool
events/gaps refuse readiness. A running-root sample does not establish future
liveness, containment or vendor validity. ACTIVE metadata remains BLOCKED with
RECEIVER_ADAPTER_UNVALIDATED, and production cannot reach it through the missing
boundary gates. Positive tests use private closed-boundary assumptions and a
managed fixture child. Session errors terminate its owned tree and record loss.
Cached stage/commit data after launch intent is historical, not pristine-worktree
or current authority proof; required checkpoint hashes remain validated. Direct
forensic restore still checks actual receiver content. No raw vendor session,
secret, model-private state or launch recipe is added to persisted readiness.

Artifact restore records immutable intent before creating files and binds any
completion receipt to the full authenticated request fingerprint. A completed
receipt permits only fresh integrity verification and read-only reuse; it cannot
mint a lease or process permit. Unrecognized or partial targets, corrupt receipts
and edited receiver files are preserved and refused. Operation IDs reserved by
intent cannot be reused for other actions or trust domains. Receipts remain local
private metadata and are not evidence of complete source fencing or power-loss
durability. See ADR-022.

Persist small append-only security audit entries for ownership changes, denied stale mutations, checkpoint corruption, export/share, recovery overrides and policy edits. Log actor/epoch/result, not sensitive tool payload by default. Provide `baton doctor` and exportable scrubbed support bundles. If compromised/corrupt state detected, freeze task and preserve evidence securely rather than automatically deleting it.

## 10. Security acceptance gates

Schema 11 tool admission requires a live borrowed native proof, exact recorded
launch cwd equal to the resolved authorized workspace, current binding/lease,
complete normalized sequence and no pending transfer or unresolved effect.
Raw JSON cannot reserve a tool or recover a consumed/lost permit. Trusted-host
ReadOnly/LocalReversible classification is not proof about arbitrary commands:
the host must constrain the concrete operation. Other effect classes and task
scopes besides `.` remain disabled. Neither Job Objects nor a cwd check prevents
all outside writes or external side effects. Late observed starts remain facts
and force recovery; they are not erased by the admission gate. QUIESCING, expiry,
abort and supervisor loss never authorize replay or revive an earlier owner.

Successive capture follows the current audited grant and receipt-bound receiver,
not the enrollment root or a submitted path. Receipt format 2 pins canonical Git
administrative identity. Only the matching task/repository and next epoch can
use the dedicated read-only capture path inside the vault. Ordinary capture
retains its overlap denial. Legacy receipts, changed Git identity and receiptless
targets are refused and preserved. This protects coordinator-controlled paths;
it does not establish isolation from another malicious process with the same
OS identity or power-loss durability. See ADR-027.

- A stale epoch is rejected in all tested mutation paths.
- MCP client cannot read artifacts of another task without permission.
- A hostile symlink/junction cannot access outside approved roots.
- Secret fixture is not exported by default and appears as an explicit gap when excluded.
- Corrupt/missing checkpoint blob blocks activation.
- An ambiguous external write is not retried.
- An untrusted repository or tool output cannot override system/operator authorization.
- Plugin disabled/time-out/`--bare` cases downgrade capability, not safety requirements.
