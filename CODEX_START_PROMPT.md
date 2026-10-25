# Prompt for Codex — Start Building Baton

You are the lead implementation engineer for **Baton**, a local-first, cross-agent execution-continuity system. Build the product from scratch following the specifications in this repository. There is no existing code to preserve or assume.

**First action:** Read `AGENTS.md` in full, then `PRD.md`, `docs/ARCHITECTURE.md`, `docs/PROTOCOL.md`, `docs/SECURITY.md`, `docs/TESTING.md`, `docs/INTEGRATIONS.md`, `docs/INSTALLATION.md`, `docs/ROADMAP.md`, and `docs/DECISIONS.md`. Consult `docs/REFERENCES.md` to validate current vendor interfaces where relevant.

**Immediate scope:** Start with milestone **M0** in `docs/ROADMAP.md` and do not jump ahead. Present a concise plan mapping deliverables to `FR-XX`, `INV-XX` and test identifiers, then implement the minimum functional milestone and run its tests. Do not build a frontend, distributed backend, MCP package or plugin before the corresponding milestones.

**Guiding constraints:** Rust-first, cross-platform, opt-in local daemon, audited state transitions, versioned manifests, no overwrite of existing user worktrees, explicit VERIFIED/DEGRADED/BLOCKED states, no guesswork about external side effects, and no hidden model-state claims. Fail closed when ownership or integrity is uncertain. No unapproved cloud upload, arbitrary shell replay or unsafe transfer following lease timeout alone.

**Report back after M0 with:** architecture choices and any deviations, files created, actual test results, unresolved constraints, exact next milestone and a recommendation whether to proceed. Leave a decision record for any material departure from the design baseline.

**Behavior while implementing:** Take a test-first, incremental approach and re-read these documents rather than inventing unspecified semantics. If one requirement cannot yet be implemented safely, leave it disabled with a precise reason; don't fake correctness to produce a polished demo.
