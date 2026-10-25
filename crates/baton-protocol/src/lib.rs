//! Versioned baseline contracts; no ownership or transfer implementation.

use serde::{Deserialize, Serialize};
use std::fmt;

mod metadata;
pub use metadata::*;
mod checkpoint;
pub use checkpoint::*;
mod execution;
pub use execution::*;
mod adapter;
pub use adapter::*;
mod local;
pub use local::*;
mod transfer;
pub use transfer::*;

/// Only the implemented schema is accepted. No implicit migration or downgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaVersion {
    #[serde(rename = "0.1.0")]
    V0_1_0,
}

/// Labels only: constructing a value does not establish a recovery guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoveryQuality {
    Verified,
    Degraded,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidInput,
    UnsupportedSchema,
    #[serde(rename = "IO_ERROR")]
    Io,
    GitUnavailable,
    GitFailed,
    Unauthorized,
    Forbidden,
    NotFound,
    StaleEpoch,
    IdempotencyConflict,
    Blocked,
    StoreBusy,
    StoreFailure,
    CorruptStore,
    IntegrityFailure,
    SourceChanged,
    UnsafePath,
    UnsupportedRepository,
    DestinationExists,
    SupervisionFailure,
    DrainTimeout,
    DaemonUnavailable,
    ResponseLost,
    ExecutionNotEnabled,
    AdapterProtocol,
    AdapterTimeout,
    AdapterRejected,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "INVALID_INPUT",
            Self::UnsupportedSchema => "UNSUPPORTED_SCHEMA",
            Self::Io => "IO_ERROR",
            Self::GitUnavailable => "GIT_UNAVAILABLE",
            Self::GitFailed => "GIT_FAILED",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::StaleEpoch => "STALE_EPOCH",
            Self::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
            Self::Blocked => "BLOCKED",
            Self::StoreBusy => "STORE_BUSY",
            Self::StoreFailure => "STORE_FAILURE",
            Self::CorruptStore => "CORRUPT_STORE",
            Self::IntegrityFailure => "INTEGRITY_FAILURE",
            Self::SourceChanged => "SOURCE_CHANGED",
            Self::UnsafePath => "UNSAFE_PATH",
            Self::UnsupportedRepository => "UNSUPPORTED_REPOSITORY",
            Self::DestinationExists => "DESTINATION_EXISTS",
            Self::SupervisionFailure => "SUPERVISION_FAILURE",
            Self::DrainTimeout => "DRAIN_TIMEOUT",
            Self::DaemonUnavailable => "DAEMON_UNAVAILABLE",
            Self::ResponseLost => "RESPONSE_LOST",
            Self::ExecutionNotEnabled => "EXECUTION_NOT_ENABLED",
            Self::AdapterProtocol => "ADAPTER_PROTOCOL",
            Self::AdapterTimeout => "ADAPTER_TIMEOUT",
            Self::AdapterRejected => "ADAPTER_REJECTED",
        }
    }

    pub const fn reason(self) -> &'static str {
        match self {
            Self::InvalidInput => "Input does not satisfy the interface contract.",
            Self::UnsupportedSchema => "The schema is not supported by this build.",
            Self::Io => "A required local I/O operation failed.",
            Self::GitUnavailable => "Git could not be found.",
            Self::GitFailed => "Git could not complete the requested operation.",
            Self::Unauthorized => "The authenticated actor does not match the request.",
            Self::Forbidden => "The actor does not have permission for this task or repository.",
            Self::NotFound => "The requested record does not exist.",
            Self::StaleEpoch => "The expected epoch does not match current authority.",
            Self::IdempotencyConflict => {
                "The operation ID was already bound to a different request."
            }
            Self::Blocked => "Unresolved ownership or lease state prevents this operation.",
            Self::StoreBusy => "The metadata store is busy.",
            Self::StoreFailure => "A required storage operation failed without acknowledgement.",
            Self::CorruptStore => "The metadata store failed integrity or record validation.",
            Self::IntegrityFailure => "A required artifact is missing, corrupt, or inconsistent.",
            Self::SourceChanged => "Workspace or index content changed during validation.",
            Self::UnsafePath => "A path or filesystem link violates the approved scope.",
            Self::UnsupportedRepository => "The repository has unsupported capture semantics.",
            Self::DestinationExists => "The receiver destination already exists.",
            Self::SupervisionFailure => "A required native process-control operation failed.",
            Self::DrainTimeout => {
                "Managed process drain did not finish within the requested bound."
            }
            Self::DaemonUnavailable => "The opt-in local coordinator is not available.",
            Self::ResponseLost => {
                "The coordinator response was not confirmed; the operation may have committed."
            }
            Self::ExecutionNotEnabled => "This build has no validated transfer activation path.",
            Self::AdapterProtocol => {
                "The managed adapter returned an invalid or unsupported protocol message."
            }
            Self::AdapterTimeout => {
                "The managed adapter did not complete within the requested bound."
            }
            Self::AdapterRejected => "The managed adapter rejected the request.",
        }
    }

    pub const fn action(self) -> &'static str {
        match self {
            Self::InvalidInput => "Correct the input before retrying.",
            Self::UnsupportedSchema => "Use a compatible build or an explicit supported migrator.",
            Self::Io => "Check local permissions and storage before retrying.",
            Self::GitUnavailable => "Install Git and make it available on PATH.",
            Self::GitFailed => "Inspect the repository and command preconditions before retrying.",
            Self::Unauthorized => "Use the capability issued to the authenticated actor.",
            Self::Forbidden => "Request explicit access from the task creator.",
            Self::NotFound => "Check the record identifier.",
            Self::StaleEpoch => "Read current state; do not execute using the old epoch.",
            Self::IdempotencyConflict => {
                "Retry the original request unchanged or use a new operation ID."
            }
            Self::Blocked => "Reconcile ownership and fencing before continuation.",
            Self::StoreBusy => "Retry the same request and operation ID after contention clears.",
            Self::StoreFailure => {
                "Inspect local storage; retry with the same operation ID to reconcile."
            }
            Self::CorruptStore => "Stop mutations and preserve the store for explicit recovery.",
            Self::IntegrityFailure => {
                "Block continuation and reconcile artifacts without overwriting evidence."
            }
            Self::SourceChanged => "Stop source writes and capture a new boundary explicitly.",
            Self::UnsafePath => "Use ordinary files and safe relative paths in an approved root.",
            Self::UnsupportedRepository => "Resolve unsupported Git state before capturing.",
            Self::DestinationExists => "Preserve the existing worktree; choose a new receiver ID.",
            Self::SupervisionFailure => {
                "Keep continuation blocked and reconcile the managed run and OS containment."
            }
            Self::DrainTimeout => {
                "Preserve run evidence and reconcile surviving processes without replaying actions."
            }
            Self::DaemonUnavailable => "Start the local daemon explicitly for this state directory.",
            Self::ResponseLost => "Inspect status or retry the same request and operation ID; do not replay processes.",
            Self::ExecutionNotEnabled => "Use metadata and forensic inspection; keep automatic continuation blocked.",
            Self::AdapterProtocol | Self::AdapterTimeout | Self::AdapterRejected => {
                "Keep continuation blocked; reconcile the managed run and inspect adapter compatibility without replaying work."
            }
        }
    }
}

/// Safe metadata only; raw paths, subprocess output and secrets are excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub schema_version: SchemaVersion,
    pub code: ErrorCode,
}

impl Diagnostic {
    pub const fn new(code: ErrorCode) -> Self {
        Self {
            schema_version: SchemaVersion::V0_1_0,
            code,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} {}",
            self.code.as_str(),
            self.code.reason(),
            self.code.action()
        )
    }
}

impl std::error::Error for Diagnostic {}
