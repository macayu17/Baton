//! M1 metadata contracts. Reservations never authorize agent execution.

use crate::{ErrorCode, SchemaVersion};
use serde::{Deserialize, Serialize};
pub use uuid::Uuid as Id;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMetadata {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskState {
    Unclaimed,
    Reserved,
    Suspect,
    Committed,
    Activating,
    Active,
    Quiescing,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub schema_version: SchemaVersion,
    pub task_id: Id,
    pub repository_id: Id,
    pub created_by: Id,
    pub goal: String,
    pub scope: String,
    pub agent: AgentMetadata,
    pub epoch: u64,
    pub last_event_sequence: u64,
    pub state: TaskState,
    pub owner_id: Option<Id>,
    pub lease_id: Option<Id>,
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    pub schema_version: SchemaVersion,
    pub task_id: Id,
    pub lease_id: Id,
    pub owner_id: Id,
    pub epoch: u64,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: SchemaVersion,
    pub operation_id: Id,
    pub actor_id: Id,
    pub expected_epoch: u64,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum Action {
    EnrollRepository {
        repository_id: Id,
        root: String,
    },
    CreateTask {
        task_id: Id,
        repository_id: Id,
        goal: String,
        scope: String,
        agent: AgentMetadata,
    },
    GrantAccess {
        task_id: Id,
        actor_id: Id,
    },
    ClaimTask {
        task_id: Id,
        lease_ms: i64,
    },
    Heartbeat {
        task_id: Id,
        lease_id: Id,
        lease_ms: i64,
    },
    CancelReservation {
        task_id: Id,
        lease_id: Id,
    },
    RecordIntent {
        task_id: Id,
        lease_id: Id,
        summary: String,
    },
    CaptureCheckpoint {
        task_id: Id,
        lease_id: Id,
        checkpoint_id: Id,
        include_untracked: Vec<String>,
    },
    RestoreCheckpoint {
        task_id: Id,
        lease_id: Id,
        checkpoint_id: Id,
        restore_id: Id,
    },
    CollectOrphans {
        task_id: Id,
        lease_id: Id,
    },
    OpenRun {
        task_id: Id,
        lease_id: Id,
        run_id: Id,
        launch_fingerprint: String,
        capabilities: crate::AdapterCapabilities,
    },
    ObserveRun {
        task_id: Id,
        lease_id: Id,
        run_id: Id,
        observation: crate::RunObservation,
    },
    RequestTransfer {
        task_id: Id,
        lease_id: Id,
        transfer_id: Id,
        receiver_id: Id,
    },
    PreflightTransfer {
        task_id: Id,
        lease_id: Id,
        transfer_id: Id,
        checkpoint_id: Id,
    },
    AbortTransfer {
        task_id: Id,
        transfer_id: Id,
    },
    BeginReceiverStaging {
        task_id: Id,
        transfer_id: Id,
        restore_id: Id,
        stage_operation_id: Id,
    },
    StageReceiver {
        task_id: Id,
        transfer_id: Id,
    },
    CommitTransfer {
        task_id: Id,
        transfer_id: Id,
        accept_degraded: bool,
        lease_ms: i64,
    },
    RecoverTransfer {
        task_id: Id,
        transfer_id: Id,
    },
    ActivateReceiver {
        task_id: Id,
        transfer_id: Id,
    },
    AcknowledgeReceiver {
        task_id: Id,
        transfer_id: Id,
        session: Box<crate::ReceiverSession>,
    },
}

impl Action {
    pub fn task_id(&self) -> Option<Id> {
        match self {
            Self::EnrollRepository { .. } => None,
            Self::CreateTask { task_id, .. }
            | Self::GrantAccess { task_id, .. }
            | Self::ClaimTask { task_id, .. }
            | Self::Heartbeat { task_id, .. }
            | Self::CancelReservation { task_id, .. }
            | Self::RecordIntent { task_id, .. }
            | Self::CaptureCheckpoint { task_id, .. }
            | Self::RestoreCheckpoint { task_id, .. }
            | Self::CollectOrphans { task_id, .. }
            | Self::OpenRun { task_id, .. }
            | Self::ObserveRun { task_id, .. }
            | Self::RequestTransfer { task_id, .. }
            | Self::PreflightTransfer { task_id, .. }
            | Self::AbortTransfer { task_id, .. }
            | Self::BeginReceiverStaging { task_id, .. }
            | Self::StageReceiver { task_id, .. }
            | Self::CommitTransfer { task_id, .. }
            | Self::RecoverTransfer { task_id, .. }
            | Self::ActivateReceiver { task_id, .. }
            | Self::AcknowledgeReceiver { task_id, .. } => Some(*task_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum Reply {
    Repository(Id),
    Task(Task),
    Lease(Lease),
    Recorded(Id),
    AccessGranted,
    Checkpoint(crate::CheckpointRecord),
    Restored(crate::RestoreReport),
    OrphansCollected(u64),
    Run(crate::RunRecord),
    Transfer(crate::TransferRecord),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventOrigin {
    Coordinator,
    AgentReported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventKind {
    TaskCreated,
    AccessGranted,
    Claimed,
    Heartbeat,
    ReservationCancelled,
    LeaseSuspect,
    ReportedIntent,
    CheckpointPublished,
    CheckpointRestored,
    RestoreReserved,
    OrphansCollected,
    RunReserved,
    ExecutionObserved,
    NativeScopeBound,
    NativeDrainSealed,
    TransferRequested,
    TransferPreflight,
    TransferAborted,
    ReceiverStagingReserved,
    ReceiverStaged,
    TransferCommitted,
    TransferCommitChecked,
    ReceiverStageChecked,
    ReceiverActivationReserved,
    ReceiverReady,
    TransferRecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub schema_version: SchemaVersion,
    pub event_id: Id,
    pub task_id: Id,
    pub sequence: u64,
    pub actor_id: Id,
    pub epoch: u64,
    pub operation_id: Id,
    pub timestamp_ms: i64,
    pub origin: EventOrigin,
    pub kind: EventKind,
    pub reported_intent: Option<String>,
    pub evidence_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditEntry {
    pub schema_version: SchemaVersion,
    pub audit_id: Id,
    pub actor_id: Id,
    pub operation_id: Id,
    pub task_id: Option<Id>,
    pub expected_epoch: u64,
    pub timestamp_ms: i64,
    pub reason: Option<ErrorCode>,
    pub kind: AuditKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuditKind {
    Request,
    LocalIdentityRecovered,
    LocalDaemonBound,
}
