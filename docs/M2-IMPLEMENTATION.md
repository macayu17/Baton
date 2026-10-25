# M2 implementation plan and evidence

Status: experimental forensic slice implemented, disabled by default. Full M2
exit gate remains open; no VERIFIED continuation is claimed.

This report records the M2 schema-2 implementation. Current M4 stores use schema
8 and refuse older stores; see `M4-IMPLEMENTATION.md` for current database behavior.

The user authorized M2 after the M1 report. Native M0 validation and the M1
production identity/ACL gaps remain pending; no release gate is waived.

## Plan and scope

1. Add versioned checkpoint manifests: repository/task/actor/operation/epoch,
   event cursor, Git base, separate index and worktree blobs, explicit policy and
   gaps (FR-05/16, INV-03). Tracer: capture staged/unstaged/binary/untracked files.
2. Add local immutable BLAKE3 CAS, bounded zstd decoding, explicit staging
   manifests, flush/hash validation, and transactional SQLite publication under
   the current authenticated reservation (FR-04/05, INV-03/07).
3. Restore only to a newly reserved engine-owned worktree using --no-checkout,
   reconstruct index separately, verify bytes/modes, and refuse all existing
   destinations, traversal/collisions, corrupt/missing blobs (FR-09, INV-05).
4. Exercise T-004/T-005 subcases: interrupted CAS writes/publication, actual
   pre/post SQLite commit kills, corruption, duplicates, stale epochs, unsupported
   Git state, ignored/secret exclusions, and reference-aware orphan collection.
   Measure fixture capture/restore timing and run full/default workspace checks.

Source writes cannot yet be fenced (M3). Double scans may detect change but do
not prove quiescence. All M2 checkpoint/restoration results therefore remain
BLOCKED for automatic continuation, with precise reasons. Artifact hash validation
is separate from a VERIFIED execution boundary. No agent activation is added.

Support regular-file content and logical index modes first. Unsupported Git
features/file kinds fail closed or appear as declared gaps; excluded files are
never checked out as an intermediate restore step. Secrets cannot be opted into
this unencrypted experimental store. Retain all immutable manifest roots during
orphan collection; do not implement destructive retention of live checkpoints.

## Implemented architecture and behavior

`baton-checkpoint` implements capture, immutable local artifacts, validation and
isolated restoration. `baton-store` authenticates requests, checks task access,
lease and epoch, and commits checkpoint roots with the event journal in SQLite.
The low-level Vault is a trusted embedding library, not an authenticated endpoint.
Both libraries remain behind the `experimental` feature; no daemon, CLI, agent
activation, network service, frontend or plugin was added.

Schema 0.1.0 manifests bind repository, task, actor, operation, epoch, event cursor
and parent checkpoint. They distinguish absent index/worktree versions using
required explicit nulls, retain original index OIDs, and record capture exclusions.
Supported regular files preserve raw bytes, staged versus unstaged changes,
deletions, staged renames, and explicitly selected nonignored untracked files.
Logical Git executable modes are retained; full ACLs, ownership, xattrs and
timestamps are not captured. Unix restores use private file/directory permissions.

BLAKE3 identifies uncompressed content; zstd is used only when it reduces size.
Decoding is bounded and hashes and lengths are revalidated. Limits are 16 MiB per
blob, 8 MiB per manifest/raw index, 256 MiB logical snapshot version data, 10,000
paths, 64 MiB Git output and 4,096 UTF-8 bytes per path. These are experimental
bounds, not a large-repository performance claim.

Capture compares two full scans of HEAD, index identity, content and gaps. A
staging manifest is flushed before blobs. Unique temporary files are flushed,
linked into CAS without replacement, then read and hash checked before manifest
promotion and transactional root publication. Unix syncs containing directories;
Windows directory-entry durability remains unproven and is recorded explicitly.
Interruption can leave orphan files, never a partially acknowledged root. A
hard-link-capable private local filesystem is required.

Restoration reserves a new UUID directory and runs Git worktree creation with
`--no-checkout --detach --lock`. It reconstructs the index through Git plumbing
and creates only included working files, then checks bytes, index OIDs/modes,
HEAD, repository identity and the complete restored path set. It never checks out
excluded base files as an intermediate step. Existing destinations and partial
receivers are preserved and refused. Restore requires the original local Git
repository and base commit; this is not a standalone repository export.

Git commands use fixed arguments without shell replay, bounded output, disabled
hooks/filters and stripped inherited Git environment. Portable path validation
rejects traversal, Windows reserved names, Git metadata aliases and normalized
case collisions. Symlinks, reparse points and hardlinks are excluded. Gitlinks,
LFS pointers, oversized files and uncertain executable metadata become declared
gaps. Sparse/split indexes, conflicts, intent-to-add, index flags and other
unsupported repository states fail closed.

The local-default-v1 policy excludes known sensitive paths/token-shaped content,
ignored files and unselected untracked files. Exclusion is whole-file, without
pretending redacted bytes are an exact snapshot. Detection is best effort, not a
guarantee that unknown secrets are absent. Sensitive inclusion and uploads are
unavailable. Existing Git history remains accessible in the shared repository.

SQLite schema version 2 stores immutable checkpoint roots. Schema-1 stores are
refused without mutation; migration and downgrade are not implemented. Exact
successful retries revalidate referenced artifacts and restored files. Collection
retains all tasks' published roots, parents, unpublished manifests and staging
references, and removes only unreferenced blobs. Checkpoint/staging retention
deletion is deferred.

ADR-018 records these scope decisions and the departure from a production durable
checkpoint boundary. No invariant has been waived: all continuation results are
BLOCKED with SUPERVISION_UNAVAILABLE, plus CAPTURE_GAPS and Windows
DURABILITY_UNPROVEN where applicable. Double scanning cannot establish writer
quiescence or detect every ABA change.

## Files created and updated

Created:

- `crates/baton-checkpoint/Cargo.toml`, `src/lib.rs`, `src/engine.rs`,
  `src/engine/git.rs`, `src/engine/tests.rs`, and `tests/snapshots.rs`.
- `crates/baton-protocol/src/checkpoint.rs`.
- `crates/baton-store/src/engine/snapshots.rs`.
- This implementation plan and evidence report.

Updated workspace manifests/lockfile, protocol exports/metadata/diagnostics and
tests, store schema/engine/tests, CI, README, architecture, protocol, testing,
roadmap, references, historical M1 report and decisions.

## Verification evidence

Environment: Windows NT 10.0.26300.0, Rust/Cargo 1.98.0 stable Windows GNU,
Git 2.51.0.windows.1. Commands use
`cargo +stable-x86_64-pc-windows-gnu`; the local MSVC linker is unavailable.

The final verification uses `--locked` throughout:

- `fmt --all -- --check`.
- `clippy --workspace --all-targets --all-features -- -D warnings`.
- `build --workspace --all-targets --all-features`.
- `test --workspace --all-features`: 48 passed, zero failed; two subprocess entry
  helpers intentionally ignored in normal discovery and explicitly invoked by
  the fault tests.
- Default-feature Clippy, all-target build and workspace tests: 11 passed, zero
  failed. This checks that experimental functionality remains disabled.

T-004/T-005 subcases cover staged/unstaged/binary/CRLF/empty/Unicode bytes,
rename/modes, explicit untracked policy, secret/ignored exclusions, native Windows
hardlink exclusion, existing destinations, source divergence, unsafe paths,
corrupt/missing artifacts, bounded compressed decoding and reference-aware GC.
Actual child-process kills cover seven CAS publication boundaries, interrupted
receiver creation and both sides of the SQLite checkpoint commit. Store tests
also cover duplicate requests, stale epochs, expiry during capture, failed SQL
publication, parent/all-task root retention and schema-1 refusal. M2-P01/P02 check
strict versions/fields and required nullable states. Existing M0/M1 tests remain
in the full run. These are subcase results, not the complete release fault matrix.

One isolated debug fixture run measured capture at 1.8687362 seconds and restore
at 578.8181 milliseconds. This tiny fixture used roughly three base files and two
selected new files; cache state was uncontrolled. It is neither a 100 MB benchmark
nor a p95 result, and does not establish the performance acceptance target.

## Unresolved constraints and next milestone

Native macOS/Linux and Windows MSVC execution, Unix symlink/permission branches,
Windows ACL validation and directory durability, power-loss/I/O fault coverage,
the full workspace matrix and representative performance remain unverified.
Three-OS CI is configured, not observed running. No encryption, OS-bound daemon
identity, process fencing, external-effect reconciliation or safe activation is
implemented. Failed receivers and staging references deliberately accumulate
until explicit reconciliation/retention work exists.

The exact next implementation milestone is **M3: Native process supervision and
safe boundaries**, targeting FR-03/06/12/14/15, INV-04/06/09 and T-006/T-007 plus
the crash matrix. Recommend proceeding with its disabled fake-agent harness and
native process-tree work while retaining all earlier gates. Do not enable
automatic continuation or call M2 production-complete before those constraints
are resolved.
