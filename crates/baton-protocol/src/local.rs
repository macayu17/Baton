//! User-scoped local control. Actor identity is supplied by the daemon's OS scope.
use crate::*;
use serde::{Deserialize, Serialize};
pub const LOCAL_REQUEST_LIMIT: usize = 64 * 1024;
pub const LOCAL_RESPONSE_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRequest {
    pub schema_version: SchemaVersion,
    pub request_id: Id,
    pub expected_epoch: u64,
    pub command: LocalCommand,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "command",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum LocalCommand {
    Invoke { action: Action },
    Status { task_id: Id },
    Tasks { after: Option<Id>, limit: u16 },
    Events { task_id: Id, after: u64, limit: u16 },
    Checkpoint { task_id: Id, checkpoint_id: Id },
    Run { task_id: Id, run_id: Id },
    Transfer { task_id: Id, transfer_id: Id },
}
/// Supervisor facts and actor creation are deliberately absent from local control.
pub fn local_mutation_allowed(action: &Action) -> bool {
    matches!(
        action,
        Action::EnrollRepository { .. }
            | Action::CreateTask { .. }
            | Action::ClaimTask { .. }
            | Action::Heartbeat { .. }
            | Action::CancelReservation { .. }
            | Action::RecordIntent { .. }
            | Action::CaptureCheckpoint { .. }
            | Action::RestoreCheckpoint { .. }
            | Action::RequestTransfer { .. }
            | Action::PreflightTransfer { .. }
            | Action::AbortTransfer { .. }
            | Action::BeginReceiverStaging { .. }
            | Action::StageReceiver { .. }
            | Action::CommitTransfer { .. }
            | Action::RecoverTransfer { .. }
            | Action::ActivateReceiver { .. }
    )
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum LocalReply {
    Mutation(Reply),
    Task(Task),
    Tasks(Vec<Task>),
    Events(Vec<Event>),
    Checkpoint(CheckpointRecord),
    Run(RunRecord),
    Transfer(TransferRecord),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalResponse {
    pub schema_version: SchemaVersion,
    pub request_id: Id,
    pub continuation_quality: RecoveryQuality,
    pub continuation_reasons: Vec<ErrorCode>,
    pub result: Result<LocalReply, Diagnostic>,
}
