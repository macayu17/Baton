//! Transfer policy model. Facts come from trusted engine checks, never client JSON.
//! Constructing a record or advancing this model grants no process authority.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferBinding {
    pub transfer_id: Id,
    pub task_id: Id,
    pub source_id: Id,
    pub receiver_id: Id,
    pub source_epoch: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferCheckpoint {
    pub checkpoint_id: Id,
    pub manifest_hash: String,
    pub event_cursor: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransferState {
    Quiescing,
    Prepared,
    Restoring,
    ReadyToCommit,
    Committed,
    Activating,
    Active,
    Blocked,
    FailedPrecommit,
    RecoveryRequired,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransferReason {
    SourceBoundaryPending,
    SourceFenceUnavailable,
    EventCoverageIncomplete,
    UnresolvedEffects,
    AdapterUnvalidated,
    ArtifactIntegrityUnverified,
    ArtifactDurabilityUnproven,
    SourceIsolationUnverified,
    ReceiverUnverified,
    ReceiverPreparationPending,
    OwnershipUncommitted,
    ActivationPending,
    OptionalContextMissing,
    DegradedAcknowledgementRequired,
    RecoveryUnresolved,
    ReceiverAdapterUnvalidated,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferRecord {
    pub schema_version: SchemaVersion,
    pub binding: TransferBinding,
    pub state: TransferState,
    pub checkpoint: Option<TransferCheckpoint>,
    pub source_drain: Option<NativeDrainRecord>,
    pub restore_id: Option<Id>,
    pub receiver_stage: Option<Box<ReceiverStage>>,
    pub receiver_activation: Option<Box<RunRecord>>,
    pub receiver_ready: Option<Box<ReceiverReadyRecord>>,
    pub committed_epoch: Option<u64>,
    pub quality: RecoveryQuality,
    pub optional_context_missing: bool,
    pub reasons: Vec<TransferReason>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiverStage {
    pub operation_id: Id,
    pub fingerprint: String,
    pub report: Option<RestoreReport>,
}

/// Necessary policy facts only; no Deserialize or client-provided attestation.
#[derive(Debug, Clone, Default)]
pub struct TransferFacts {
    pub source_fenced: bool,
    pub events_complete: bool,
    pub effects_settled: bool,
    pub adapter_validated: bool,
    pub artifacts_verified: bool,
    pub artifacts_durable: bool,
    pub source_isolated: bool,
    pub receiver_verified: bool,
    pub receiver_started: bool,
    pub optional_context_missing: bool,
}
pub enum TransferStep {
    Prepare(TransferCheckpoint),
    BeginRestore,
    ReceiverValidated { restore_id: Id },
    Commit { accept_degraded: bool },
    Activate,
    ReceiverStarted,
    Abort,
    Recover,
}
impl TransferFacts {
    pub fn missing(&self, receiver: bool) -> Vec<TransferReason> {
        let mut reasons = Vec::new();
        for (present, reason) in [
            (self.source_fenced, TransferReason::SourceFenceUnavailable),
            (
                self.events_complete,
                TransferReason::EventCoverageIncomplete,
            ),
            (self.effects_settled, TransferReason::UnresolvedEffects),
            (self.adapter_validated, TransferReason::AdapterUnvalidated),
            (
                self.artifacts_verified,
                TransferReason::ArtifactIntegrityUnverified,
            ),
            (
                self.artifacts_durable,
                TransferReason::ArtifactDurabilityUnproven,
            ),
            (
                self.source_isolated,
                TransferReason::SourceIsolationUnverified,
            ),
        ] {
            if !present {
                reasons.push(reason);
            }
        }
        if receiver && !self.receiver_verified {
            reasons.push(TransferReason::ReceiverUnverified);
        }
        reasons
    }
}
impl TransferRecord {
    pub fn requested(binding: TransferBinding) -> Result<Self, Diagnostic> {
        if [
            binding.transfer_id,
            binding.task_id,
            binding.source_id,
            binding.receiver_id,
        ]
        .iter()
        .any(Id::is_nil)
            || binding.source_epoch == 0
            || binding.source_epoch >= i64::MAX as u64
        {
            return Err(Diagnostic::new(ErrorCode::InvalidInput));
        }
        Ok(Self {
            schema_version: SchemaVersion::V0_1_0,
            binding,
            state: TransferState::Quiescing,
            checkpoint: None,
            source_drain: None,
            restore_id: None,
            receiver_stage: None,
            receiver_activation: None,
            receiver_ready: None,
            committed_epoch: None,
            quality: RecoveryQuality::Blocked,
            optional_context_missing: false,
            reasons: vec![TransferReason::SourceBoundaryPending],
        })
    }
    pub fn advance(
        &mut self,
        actor: Id,
        epoch: u64,
        step: TransferStep,
        facts: &TransferFacts,
    ) -> Result<(), Diagnostic> {
        let mut next = self.clone();
        next.transition(actor, epoch, step, facts)?;
        *self = next;
        Ok(())
    }
    fn transition(
        &mut self,
        actor: Id,
        epoch: u64,
        step: TransferStep,
        facts: &TransferFacts,
    ) -> Result<(), Diagnostic> {
        if epoch != self.committed_epoch.unwrap_or(self.binding.source_epoch) {
            return Err(Diagnostic::new(ErrorCode::StaleEpoch));
        }
        let source_operation = matches!(step, TransferStep::Prepare(_) | TransferStep::Abort)
            || (matches!(step, TransferStep::Recover) && self.committed_epoch.is_none());
        let expected_actor = if source_operation {
            self.binding.source_id
        } else {
            self.binding.receiver_id
        };
        if actor != expected_actor {
            return Err(Diagnostic::new(ErrorCode::Forbidden));
        }
        let invalid = || Diagnostic::new(ErrorCode::InvalidInput);
        match step {
            TransferStep::Abort => {
                if self.committed_epoch.is_some() || self.state == TransferState::FailedPrecommit {
                    return Err(invalid());
                }
                self.state = TransferState::FailedPrecommit;
                self.quality = RecoveryQuality::Blocked;
                self.reasons = vec![TransferReason::RecoveryUnresolved];
                return Ok(());
            }
            TransferStep::Recover => {
                if self.state == TransferState::FailedPrecommit {
                    return Err(invalid());
                }
                self.state = TransferState::RecoveryRequired;
                self.quality = RecoveryQuality::Blocked;
                self.reasons = vec![TransferReason::RecoveryUnresolved];
                return Ok(());
            }
            TransferStep::Prepare(checkpoint) => {
                if self.state != TransferState::Quiescing
                    || checkpoint.checkpoint_id.is_nil()
                    || !valid_evidence_hash(&checkpoint.manifest_hash)
                {
                    return Err(invalid());
                }
                self.checkpoint = Some(checkpoint);
                self.state = TransferState::Prepared;
            }
            TransferStep::BeginRestore => {
                if self.state != TransferState::Prepared {
                    return Err(invalid());
                }
                self.state = TransferState::Restoring;
            }
            TransferStep::ReceiverValidated { restore_id } => {
                if self.state != TransferState::Restoring || restore_id.is_nil() {
                    return Err(invalid());
                }
                self.restore_id = Some(restore_id);
                self.state = TransferState::ReadyToCommit;
            }
            TransferStep::Commit { accept_degraded } => {
                if self.state != TransferState::ReadyToCommit {
                    return Err(invalid());
                }
                if (self.optional_context_missing || facts.optional_context_missing)
                    && !accept_degraded
                    && facts.missing(true).is_empty()
                {
                    self.reasons = vec![
                        TransferReason::OptionalContextMissing,
                        TransferReason::DegradedAcknowledgementRequired,
                    ];
                    return Ok(());
                }
                self.optional_context_missing |= facts.optional_context_missing;
                self.committed_epoch = Some(self.binding.source_epoch + 1);
                self.state = TransferState::Committed;
            }
            TransferStep::Activate => {
                if self.state != TransferState::Committed {
                    return Err(invalid());
                }
                self.state = TransferState::Activating;
            }
            TransferStep::ReceiverStarted => {
                if self.state != TransferState::Activating || !facts.receiver_started {
                    return Err(invalid());
                }
                self.state = TransferState::Active;
            }
        }
        let needs_receiver = matches!(
            self.state,
            TransferState::ReadyToCommit
                | TransferState::Committed
                | TransferState::Activating
                | TransferState::Active
        );
        let missing = facts.missing(needs_receiver);
        if !missing.is_empty() {
            // A failed precommit check must not allocate an epoch, even in the model.
            if self.state == TransferState::Committed {
                self.committed_epoch = None;
            }
            self.state = if self.committed_epoch.is_some() {
                TransferState::RecoveryRequired
            } else {
                TransferState::Blocked
            };
            self.quality = RecoveryQuality::Blocked;
            self.reasons = missing;
            return Ok(());
        }
        if self.committed_epoch.is_some()
            && facts.optional_context_missing
            && !self.optional_context_missing
        {
            self.state = TransferState::RecoveryRequired;
            self.quality = RecoveryQuality::Blocked;
            self.reasons = vec![
                TransferReason::OptionalContextMissing,
                TransferReason::DegradedAcknowledgementRequired,
            ];
            return Ok(());
        }
        self.optional_context_missing |= facts.optional_context_missing;
        self.reasons = match self.state {
            TransferState::Prepared | TransferState::Restoring => {
                vec![TransferReason::ReceiverPreparationPending]
            }
            TransferState::ReadyToCommit => vec![TransferReason::OwnershipUncommitted],
            TransferState::Committed | TransferState::Activating => {
                vec![TransferReason::ActivationPending]
            }
            _ => Vec::new(),
        };
        if self.optional_context_missing {
            self.reasons.push(TransferReason::OptionalContextMissing);
        }
        self.quality = if self.state != TransferState::Active {
            RecoveryQuality::Blocked
        } else if self.optional_context_missing {
            RecoveryQuality::Degraded
        } else {
            RecoveryQuality::Verified
        };
        Ok(())
    }
}
