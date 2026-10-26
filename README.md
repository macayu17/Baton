# Baton

Baton saves local checkpoints and event logs for coding tasks. The goal is to let another AI agent continue unfinished work with evidence of what happened. The first planned handoff is Codex to Claude Code.

**This is an experimental development build.** The CLI can manage tasks, capture workspace checkpoints and restore captured files into a new Git worktree. Agent launching and automatic handoff are still under development.

## What works today

- Enroll a Git repository without changing its files
- Create tasks, reserve ownership and inspect their history
- Save staged and unstaged changes, plus explicitly included untracked files
- Verify captured file contents and restore them into a new worktree
- Inspect task status, checkpoints and event history through the CLI or JSON

Snapshots stay local. Restoration keeps the original checkout intact and refuses to overwrite existing work. Checkpoint reports include exclusion and support warnings.

## Build locally

You'll need:

- Rust stable, including Cargo, rustfmt and Clippy
- Git
- A C/C++ build toolchain for native dependencies

On Windows, use Visual Studio C++ Build Tools or a configured Rust GNU toolchain. Linux needs a C compiler and linker; macOS needs the Xcode Command Line Tools. Native runtime testing so far has been on Windows; Linux and macOS validation is incomplete.

Clone and build the CLI:

```sh
git clone https://github.com/macayu17/Baton.git
cd Baton
cargo build -p baton-cli --features experimental --locked
```

The executable is `target/debug/baton`, or `target/debug/baton.exe` on Windows. The examples below use the Unix path; in PowerShell, use `.\target\debug\baton.exe` instead.

With a configured Windows GNU toolchain, use `cargo +stable-x86_64-pc-windows-gnu` in place of `cargo`.

The `experimental` feature is required to build the executable. The local CLI walkthrough does not require agent installations or API credentials.

## Run the CLI

Run these commands from the cloned Baton directory. This example enrolls that checkout; replace `--repo .` with another Git repository's path to use your own project. Checkpoint capture requires a repository with at least one commit.

Choose an unused state directory outside all Git repositories. Its parent must already exist and belong to your account. Let Baton create the state directory:

```sh
./target/debug/baton --state "$HOME/baton-state" init --repo .
```

Start the daemon in the foreground:

```sh
./target/debug/baton --state "$HOME/baton-state" daemon
```

Keep that terminal open. From a second terminal in the Baton directory, inspect the state:

```sh
./target/debug/baton --state "$HOME/baton-state" status
./target/debug/baton --state "$HOME/baton-state" --json status
```

Stop the daemon with Ctrl+C. Initialization does not start it automatically, and these commands do not install a background service or change global agent configuration.

If startup reports `DAEMON_UNAVAILABLE`, start the daemon with the same `--state` path. If Baton refuses directory permissions, choose a location owned by your account. Development builds do not migrate older databases; keep the old state and use a new directory if its schema is unsupported.

## Use tasks and checkpoints

Every command uses the same `--state` directory. Run a subcommand with `--help` for its arguments:

```sh
./target/debug/baton create-task --help
./target/debug/baton checkpoint --help
./target/debug/baton restore --help
```

The current workflow is:

| Command | What it does |
| --- | --- |
| `init --repo path` | Enroll a repository and return its ID |
| `create-task` | Create a task with a goal and return its ID |
| `claim --task task_id` | Reserve ownership and return a lease ID and epoch |
| `checkpoint` | Capture the task's current workspace |
| `checkpoint-info` | Inspect a checkpoint and its limitations |
| `restore` | Restore captured files into a new worktree |
| `status` | Inspect one task or list tasks |
| `events --task task_id` | Read a task's event history |

Use the IDs returned by earlier commands. Ownership-sensitive commands need the current `--epoch` and lease ID. Claims expire after five minutes by default; `heartbeat` renews a live claim. Reuse `--operation-id` when retrying the same request after a lost response.

Untracked files require explicit inclusion through `checkpoint --include-untracked path`. Ignored files and sensitive content can be excluded, so inspect the checkpoint report before relying on a restore. The restore result includes the new worktree's path.

**Agent continuation currently reports `BLOCKED`.** A checkpoint or restore can succeed while continuation remains blocked. Use `--json` to inspect the command's `result` separately from `continuation_quality`. Lease expiry never starts a replacement agent, and uncertain external operations are not replayed automatically.

## Development

Run the workspace checks, including the experimental engines:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

Vendor probes are opt-in and ignored by the ordinary suite. The [M5 implementation report](docs/M5-IMPLEMENTATION.md) records their results, including the unresolved tool-inventory check. Passing local tests does not establish a working cross-agent handoff.

For design and implementation details:

- [Roadmap](docs/ROADMAP.md): current work and remaining milestones
- [Architecture](docs/ARCHITECTURE.md): module and storage boundaries
- [Protocol](docs/PROTOCOL.md) and [security](docs/SECURITY.md): ownership, recovery and safety rules
- [Testing](docs/TESTING.md): acceptance tests and release gates
- [Product requirements](PRD.md): intended behavior and scope
- [Agent instructions](AGENTS.md): requirements for changing the code
