# Installation, Packaging and Distribution

## 1. Decision: install a native product, then optional integrations

**Recommendation: native Rust CLI/daemon + local stdio MCP gateway + vendor plugins/skills.** MCP is a tool protocol, not a process supervisor, durable store or kernel sandbox. Plugins bundle installable instructions, hooks and MCP configuration, but cannot reliably substitute for Baton-managed agents and storage. The cloud/team service remains optional.

### Product layers

| Layer | Installable thing | Mandatory? | Function |
|---|---|---|---|
| Native core | `baton` CLI and `batond` service (or single binary with daemon mode) | Yes | Workspace capture, leases, CAS, process supervisor, recovery |
| Local MCP | `baton mcp serve` backed by daemon | Recommended | Agent-controlled status, evidence and permitted transfer requests |
| Codex integration | Agent plugin/skills + integration enrollment | Optional for CLI, recommended for seamless UX | Setup, workflows, optional hooks where supported |
| Claude integration | Claude Code plugin and hooks + enrollment | Optional for CLI, recommended for seamless UX | Lifecycle hooks, provider workflow integration |
| Team sync (R2) | Authenticated client option; hosted/self-hosted server | No | Cross-machine artifacts, task authority, permissions |
| GUI | Local lightweight dashboard / editor extension | No | Timeline and recovery explanation only |

## 2. Desired onboarding UX

The end-user should be able to install signed `baton` through a familiar OS package manager, confirm Git/Codex/Claude availability with `baton doctor`, initialize an existing repository using `baton init`, and enable individual adapters interactively. Do not modify global agent config, `AGENTS.md`, or hooks without explicit consent and reversible backup. Show a safety warning when users opt into capturing command outputs or ignored files.

**Proposed UX commands (not currently implemented):**

- `baton install-adapter codex` — help enroll managed Codex runtime and optionally install package/instructions.
- `baton install-adapter claude` — configure Claude Code hooks/MCP and verify installed plugin version.
- `baton init` — attach a repository without modifying existing content.
- `baton doctor` — verify repo, process controls, daemon, agent interfaces, Git, database, permissions, encryption and plugin installation.
- `baton start --agent codex` — launch a task-supervised agent.
- `baton transfer --to claude` — request ownership handoff.
- `baton uninstall` — remove global integration, daemon and optionally local Baton-owned data, never user project files implicitly.

Prefer **no elevated admin privilege** for normal operation. Installation must offer user-scoped service/launch-agent/task scheduler setup. Config and data locations should follow OS conventions with overrides; do not store absolute Windows-only paths in portable manifests.

## 3. Platform packaging

- **Windows:** signed native release binaries and installer; `winget` first, Scoop/community option later. Manage child processes via Job Objects and local IPC via named pipes. Test PowerShell, Windows Git and path casing.
- **macOS:** signed/notarized universal or architecture-specific builds where feasible; Homebrew tap; user-scoped launch agent if needed.
- **Linux:** signed checksummed binaries, package manager artifacts and Homebrew/Linuxbrew option; systemd --user where available, otherwise supervised on-demand mode.
- **Developer install:** Cargo may be offered, but production users should not need a Rust toolchain or Node/Python.

Release gates: hashes, signed builds, artifact SBOM, repeatable CI on three OS families, smoke tests, rollback and schema compatibility. Do not auto-start a background daemon or capture activity before explicit user enrollment.

## 4. MCP packaging

Provide the MCP server using **stdio** as a native binary subcommand. It connects to a per-user daemon via authenticated local IPC, without exposing a LAN port by default. Each host registers the MCP endpoint using its host-specific configuration. Remote HTTP MCP is useful only when a team server deliberately exposes authenticated read/control operations; it must not let remote agents mutate a user's local filesystem without a device-side authorization handshake.

MCP supports task queries, evidence retrieval and *requests* to coordinate transfer. Actual capture, process interruption, read/write isolation and checkpoint integrity remain native daemon responsibilities.

## 5. Codex plugin distribution

OpenAI's plugin system supports portable root `plugin.json`, optional root `mcp.json`, bundled `skills/`, and optional hooks depending on the runtime. **Version-sensitive caveat:** as documented in October 2026, lifecycle-hook-bearing Codex desktop plugins are available for manual installation but not the public plugin directory. Therefore:

- Ship **the native installer as the real installation**.
- Offer a **directory-compatible skills/MCP plugin** for public discovery once eligible.
- Offer trusted local hook enrollment via installer/manual steps for supported Codex versions; publish separate hookless package if needed.
- Do not claim installing a Codex plugin on the web deployed local binaries or security hooks.
- Use official package schemas rather than inventing a proprietary plugin manifest. Test import/marketplace flow with supported versions.

## 6. Claude Code plugin distribution

Claude Code supports `.claude-plugin/plugin.json` with skills, hooks, agents and MCP configs. Use marketplace distribution when stable; before that use a local plugin folder for tests. Explicitly provide activation, uninstall, plugin validation and failure diagnosis. Hooks must never silently weaken the user's approval settings, and must only attach to a verified Baton task workspace.

Never assume Claude `--bare` mode includes normal plugin/hook/MCP instrumentation; it intentionally skips it. If bare mode is required for CI, supply an independent enforcement/observability mechanism and prove equivalent guarantees.

## 7. Installation options and tradeoffs

| Option | Advantage | Limitation | Recommendation |
|---|---|---|---|
| Native CLI + daemon | Full local process/storage access, fast, cross-platform | Installer and OS maintenance | **Required core** |
| MCP only | Easy connection to different agents | Cannot own processes or guarantee file fencing | **Never sole product** |
| Plugins only | Great discoverability and setup UX | Host-dependent hooks and security limits | **Wrapper over core** |
| IDE extension | Familiar editor UX | IDE-specific; incomplete CLI/headless coverage | Later |
| Hosted SaaS only | Easy remote coordination | Trust, privacy, local filesystem and offline constraints | Not first |

## 8. Uninstall and upgrades

- `baton doctor` reports active task ownership and pending recovery before upgrade.
- Schema migrations must be transactional, reversible where possible, and block downgrade if checkpoint format would be corrupted.
- Uninstall removes configured hooks/MCP entries only if Baton installed them and leaves a backup for user-modified files.
- Prompt separately for deletion of Baton metadata, encrypted artifacts and keys. User code and unrelated Git worktrees are not deleted without explicit scoped confirmation.
- Detect orphan tasks/processes before removal; do not terminate arbitrary processes.

## 9. Suggested packaging milestones

**R0** directly runnable debug CLI/daemon. **R1a** native signed release and setup doctor. **R1b** stdio MCP and Codex/Claude local packages. **R1c** package managers and optional marketplace submission. **R2** team server installer/docker image and per-machine pairing.

See `REFERENCES.md` for official OpenAI/Anthropic plugin and MCP documentation; distribution policies can change, so revalidate before release.
