use baton_protocol::*;
use clap::{Parser, Subcommand};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    name = "baton",
    version,
    about = "Baton experimental local continuity engine"
)]
struct Args {
    #[arg(long, global = true)]
    state: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    operation_id: Option<Id>,
    #[arg(long, global = true, default_value_t = 0)]
    epoch: u64,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Enroll a repository without changing its files. Creates new private state.
    Init {
        #[arg(long)]
        repo: PathBuf,
    },
    /// Run the opt-in native coordinator in the foreground.
    Daemon,
    CreateTask {
        #[arg(long)]
        repository: Id,
        #[arg(long)]
        task_id: Option<Id>,
        #[arg(long)]
        goal: String,
        #[arg(long, default_value = ".")]
        scope: String,
        #[arg(long, default_value = "unspecified")]
        agent: String,
        #[arg(long, default_value = "unvalidated")]
        agent_version: String,
    },
    /// Reserve metadata ownership; this grants no agent process activation.
    Claim {
        #[arg(long)]
        task: Id,
        #[arg(long, default_value_t = 300_000)]
        lease_ms: i64,
    },
    Heartbeat {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        lease: Id,
        #[arg(long, default_value_t = 300_000)]
        lease_ms: i64,
    },
    Status {
        #[arg(long)]
        task: Option<Id>,
        #[arg(long)]
        after: Option<Id>,
        #[arg(long, default_value_t = 100)]
        limit: u16,
    },
    Events {
        #[arg(long)]
        task: Id,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 100)]
        limit: u16,
    },
    /// Capture local forensic artifacts. Continuation remains BLOCKED.
    Checkpoint {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        lease: Id,
        #[arg(long)]
        checkpoint_id: Option<Id>,
        #[arg(long)]
        include_untracked: Vec<String>,
    },
    /// Restore only in a new owned worktree. Does not activate an agent.
    Restore {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        lease: Id,
        #[arg(long)]
        checkpoint: Id,
        #[arg(long)]
        restore_id: Option<Id>,
    },
    /// Record transfer intent. Does not stop the source or activate a receiver.
    TransferRequest {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        lease: Id,
        #[arg(long)]
        receiver: Id,
        #[arg(long)]
        transfer_id: Option<Id>,
    },
    /// Report unmet transfer gates against a checkpoint. Currently always BLOCKED.
    TransferPreflight {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        lease: Id,
        #[arg(long)]
        transfer: Id,
        #[arg(long)]
        checkpoint: Id,
    },
    /// Reserve receiver staging. Requires an engine-verified PREPARED transfer.
    TransferBeginStaging {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        transfer: Id,
        #[arg(long)]
        restore_id: Option<Id>,
        #[arg(long)]
        stage_operation_id: Id,
    },
    /// Complete staging using its reserved --operation-id; grants no writer authority.
    TransferStage {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        transfer: Id,
    },
    /// Commit ownership after fresh source/receiver checks. Does not activate an agent.
    TransferCommit {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        transfer: Id,
        #[arg(long)]
        accept_degraded: bool,
        #[arg(long, default_value_t = 300_000)]
        lease_ms: i64,
    },
    /// Record recovery required without rolling back committed ownership.
    TransferRecover {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        transfer: Id,
    },
    /// Disabled until a validated receiver activation path is implemented.
    TransferActivate {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        transfer: Id,
    },
    TransferAbort {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        transfer: Id,
    },
    TransferInfo {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        transfer: Id,
    },
    CheckpointInfo {
        #[arg(long)]
        task: Id,
        #[arg(long)]
        checkpoint: Id,
    },
}
fn absolute(path: &Path) -> Result<PathBuf, Diagnostic> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|_| Diagnostic::new(ErrorCode::Io))
    }
}
fn output(response: &LocalResponse, json: bool) -> ExitCode {
    if json {
        if let Ok(text) = serde_json::to_string(response) {
            let _ = writeln!(std::io::stdout(), "{text}");
        }
    } else {
        let _ = writeln!(
            std::io::stdout(),
            "Operation: {}\nContinuation: BLOCKED (EXECUTION_NOT_ENABLED)",
            response.request_id
        );
        match &response.result {
            Ok(reply) => {
                if let Ok(text) = serde_json::to_string_pretty(reply) {
                    let _ = writeln!(std::io::stdout(), "{text}");
                }
            }
            Err(diagnostic) => {
                let _ = writeln!(std::io::stdout(), "{diagnostic}");
            }
        }
    }
    if response.result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(err)
            if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            let _ = err.print();
            return ExitCode::SUCCESS;
        }
        Err(_) => {
            return output(
                &baton_cli::response(Id::nil(), Err(Diagnostic::new(ErrorCode::InvalidInput))),
                std::env::args_os().any(|s| s == "--json"),
            )
        }
    };
    let id = args.operation_id.unwrap_or_else(Id::new_v4);
    if matches!(args.command, Command::TransferStage { .. }) && args.operation_id.is_none() {
        return output(
            &baton_cli::response(id, Err(Diagnostic::new(ErrorCode::InvalidInput))),
            args.json,
        );
    }
    let Some(state) = &args.state else {
        return output(
            &baton_cli::response(id, Err(Diagnostic::new(ErrorCode::InvalidInput))),
            args.json,
        );
    };
    let root = match absolute(state) {
        Ok(root) => root,
        Err(e) => return output(&baton_cli::response(id, Err(e)), args.json),
    };
    if matches!(args.command, Command::Daemon) {
        let result = baton_cli::serve(&root, |session| {
            let _ = writeln!(std::io::stdout(), "{}", serde_json::json!({"schema_version":"0.1.0","state":"LISTENING","session_id":session}));
        }).await;
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => output(&baton_cli::response(id, Err(e)), args.json),
        };
    }
    if let Command::Init { repo } = &args.command {
        let result = absolute(repo).and_then(|repo| baton_cli::initialize(&root, &repo, id));
        return output(&baton_cli::response(id, result), args.json);
    }
    let command = match args.command {
        Command::CreateTask {
            repository,
            task_id,
            goal,
            scope,
            agent,
            agent_version,
        } => LocalCommand::Invoke {
            action: Action::CreateTask {
                task_id: task_id.unwrap_or(id),
                repository_id: repository,
                goal,
                scope,
                agent: AgentMetadata {
                    name: agent,
                    version: agent_version,
                },
            },
        },
        Command::Claim { task, lease_ms } => LocalCommand::Invoke {
            action: Action::ClaimTask {
                task_id: task,
                lease_ms,
            },
        },
        Command::Heartbeat {
            task,
            lease,
            lease_ms,
        } => LocalCommand::Invoke {
            action: Action::Heartbeat {
                task_id: task,
                lease_id: lease,
                lease_ms,
            },
        },
        Command::Status {
            task: Some(task_id),
            ..
        } => LocalCommand::Status { task_id },
        Command::Status { after, limit, .. } => LocalCommand::Tasks { after, limit },
        Command::Events { task, after, limit } => LocalCommand::Events {
            task_id: task,
            after,
            limit,
        },
        Command::Checkpoint {
            task,
            lease,
            checkpoint_id,
            include_untracked,
        } => LocalCommand::Invoke {
            action: Action::CaptureCheckpoint {
                task_id: task,
                lease_id: lease,
                checkpoint_id: checkpoint_id.unwrap_or(id),
                include_untracked,
            },
        },
        Command::Restore {
            task,
            lease,
            checkpoint,
            restore_id,
        } => LocalCommand::Invoke {
            action: Action::RestoreCheckpoint {
                task_id: task,
                lease_id: lease,
                checkpoint_id: checkpoint,
                restore_id: restore_id.unwrap_or(id),
            },
        },
        Command::TransferRequest {
            task,
            lease,
            receiver,
            transfer_id,
        } => LocalCommand::Invoke {
            action: Action::RequestTransfer {
                task_id: task,
                lease_id: lease,
                receiver_id: receiver,
                transfer_id: transfer_id.unwrap_or(id),
            },
        },
        Command::TransferPreflight {
            task,
            lease,
            transfer,
            checkpoint,
        } => LocalCommand::Invoke {
            action: Action::PreflightTransfer {
                task_id: task,
                lease_id: lease,
                transfer_id: transfer,
                checkpoint_id: checkpoint,
            },
        },
        Command::TransferBeginStaging {
            task,
            transfer,
            restore_id,
            stage_operation_id,
        } => LocalCommand::Invoke {
            action: Action::BeginReceiverStaging {
                task_id: task,
                transfer_id: transfer,
                restore_id: restore_id.unwrap_or(id),
                stage_operation_id,
            },
        },
        Command::TransferStage { task, transfer } => LocalCommand::Invoke {
            action: Action::StageReceiver {
                task_id: task,
                transfer_id: transfer,
            },
        },
        Command::TransferCommit {
            task,
            transfer,
            accept_degraded,
            lease_ms,
        } => LocalCommand::Invoke {
            action: Action::CommitTransfer {
                task_id: task,
                transfer_id: transfer,
                accept_degraded,
                lease_ms,
            },
        },
        Command::TransferRecover { task, transfer } => LocalCommand::Invoke {
            action: Action::RecoverTransfer {
                task_id: task,
                transfer_id: transfer,
            },
        },
        Command::TransferActivate { task, transfer } => LocalCommand::Invoke {
            action: Action::ActivateReceiver {
                task_id: task,
                transfer_id: transfer,
            },
        },
        Command::TransferAbort { task, transfer } => LocalCommand::Invoke {
            action: Action::AbortTransfer {
                task_id: task,
                transfer_id: transfer,
            },
        },
        Command::TransferInfo { task, transfer } => LocalCommand::Transfer {
            task_id: task,
            transfer_id: transfer,
        },
        Command::CheckpointInfo { task, checkpoint } => LocalCommand::Checkpoint {
            task_id: task,
            checkpoint_id: checkpoint,
        },
        Command::Init { .. } | Command::Daemon => unreachable!("handled above"),
    };
    let request = LocalRequest {
        schema_version: SchemaVersion::V0_1_0,
        request_id: id,
        expected_epoch: args.epoch,
        command,
    };
    let result = baton_cli::request(root, request)
        .await
        .unwrap_or_else(|e| baton_cli::response(id, Err(e)));
    output(&result, args.json)
}
