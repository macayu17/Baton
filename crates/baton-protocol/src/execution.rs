//! Observable run facts. Serialized records never grant process or writer authority.
use crate::{AgentMetadata, Diagnostic, ErrorCode, Id, SchemaVersion};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBinding {
    pub run_id: Id,
    pub task_id: Id,
    pub actor_id: Id,
    pub lease_id: Id,
    pub epoch: u64,
}

/// Audited native scope identity; serialized records are not live proof objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeScopeRecord {
    pub schema_version: SchemaVersion,
    pub binding: RunBinding,
    pub scope_id: Id,
    pub launch_fingerprint: String,
    pub pid: u32,
    pub launch_worktree: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDrainRecord {
    pub schema_version: SchemaVersion,
    pub scope: NativeScopeRecord,
    pub observed_source_cursor: u64,
    pub execution_sequence: u64,
}

/// Normalized adapter acknowledgement. Its JSON alone is not readiness authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiverSession {
    pub schema_version: SchemaVersion,
    pub binding: RunBinding,
    pub session_id: Id,
    pub adapter: AgentMetadata,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiverReadyRecord {
    pub schema_version: SchemaVersion,
    pub scope: NativeScopeRecord,
    pub session: ReceiverSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterCapabilities {
    pub lifecycle: bool,
    pub tool_outcomes: bool,
    pub interrupt: bool,
    pub mutation_gating: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectClass {
    ReadOnly,
    LocalReversible,
    LocalUncertain,
    ExternalVerifiable,
    ExternalNonIdempotent,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ToolOutcome {
    Succeeded,
    Failed,
    Interrupted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectState {
    Reserved,
    Started,
    Completed,
    Uncertain,
    Reconciled,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectRecord {
    pub effect_id: Id,
    pub class: EffectClass,
    pub state: EffectState,
    pub outcome: Option<ToolOutcome>,
    pub reconciliation_hash: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RunState {
    LaunchPending,
    Running,
    Stopping,
    Stopped,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum RunObservation {
    StartupConfigReported {
        source_sequence: u64,
        request_id: crate::AdapterRequestId,
        payload_hash: String,
        services_present: bool,
    },
    RemoteControlDisabled {
        source_sequence: u64,
        payload_hash: String,
    },
    ThreadAnnounced {
        source_sequence: u64,
        thread_id: String,
        payload_hash: String,
    },
    ThreadRegistered {
        source_sequence: u64,
        effect_id: Id,
        request_id: crate::AdapterRequestId,
        payload_hash: String,
        thread_id: String,
    },
    Spawned {
        pid: u32,
    },
    InterruptRequested,
    InterruptAcknowledged {
        source_sequence: u64,
    },
    ToolReserved {
        source_sequence: u64,
        effect_id: Id,
        class: EffectClass,
    },
    ToolStarted {
        source_sequence: u64,
        effect_id: Id,
        class: EffectClass,
    },
    ToolFinished {
        source_sequence: u64,
        effect_id: Id,
        outcome: ToolOutcome,
    },
    TreeStopped,
    SupervisorLost,
    AdapterReported {
        source_sequence: u64,
        event: crate::AdapterEvent,
    },
    AdapterGap {
        source_sequence: u64,
    },
    EffectReconciled {
        effect_id: Id,
        evidence_hash: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BoundaryReason {
    LaunchAmbiguous,
    ProcessTreeUnconfirmed,
    SupervisorLost,
    UnresolvedEffect,
    EventGap,
    AdapterUnsupported,
    AdapterUnvalidated,
    MutationGatingUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    pub schema_version: SchemaVersion,
    pub binding: RunBinding,
    pub launch_fingerprint: String,
    pub capabilities: AdapterCapabilities,
    pub state: RunState,
    pub pid: Option<u32>,
    pub source_thread: Option<String>,
    pub announced_thread: Option<String>,
    pub source_cursor: u64,
    pub event_gap: bool,
    pub tree_stopped: bool,
    pub effects: Vec<EffectRecord>,
}

impl RunRecord {
    pub fn pending(
        binding: RunBinding,
        launch_fingerprint: String,
        capabilities: AdapterCapabilities,
    ) -> Self {
        Self {
            schema_version: SchemaVersion::V0_1_0,
            binding,
            launch_fingerprint,
            capabilities,
            state: RunState::LaunchPending,
            pid: None,
            source_thread: None,
            announced_thread: None,
            source_cursor: 0,
            event_gap: false,
            tree_stopped: false,
            effects: Vec::new(),
        }
    }

    /// Invalid observations leave the prior record unchanged.
    pub fn apply(&mut self, observation: &RunObservation) -> Result<(), Diagnostic> {
        let mut next = self.clone();
        next.transition(observation)?;
        *self = next;
        Ok(())
    }

    fn transition(&mut self, observation: &RunObservation) -> Result<(), Diagnostic> {
        let invalid = || Diagnostic::new(ErrorCode::InvalidInput);
        if let RunObservation::AdapterReported {
            source_sequence,
            event,
        } = observation
        {
            event.validate()?;
            match &event.event {
                crate::AdapterEventKind::ToolStarted { effect_id, .. } => {
                    return self.transition(&RunObservation::ToolStarted {
                        source_sequence: *source_sequence,
                        effect_id: *effect_id,
                        class: EffectClass::Unknown,
                    });
                }
                crate::AdapterEventKind::ToolCompleted {
                    effect_id, outcome, ..
                } => {
                    if self.effects.iter().any(|effect| {
                        effect.effect_id == *effect_id && effect.class != EffectClass::Unknown
                    }) {
                        return Err(invalid());
                    }
                    if !self
                        .effects
                        .iter()
                        .any(|effect| effect.effect_id == *effect_id)
                    {
                        if self.effects.len() >= 4096 {
                            return Err(invalid());
                        }
                        self.event_gap = true;
                        self.effects.push(EffectRecord {
                            effect_id: *effect_id,
                            class: EffectClass::Unknown,
                            state: EffectState::Started,
                            outcome: None,
                            reconciliation_hash: None,
                        });
                    }
                    return self.transition(&RunObservation::ToolFinished {
                        source_sequence: *source_sequence,
                        effect_id: *effect_id,
                        outcome: *outcome,
                    });
                }
                _ => (),
            }
        }
        let sequence = match observation {
            RunObservation::StartupConfigReported {
                source_sequence, ..
            } => Some(*source_sequence),
            RunObservation::RemoteControlDisabled {
                source_sequence, ..
            } => Some(*source_sequence),
            RunObservation::ThreadAnnounced {
                source_sequence, ..
            } => Some(*source_sequence),
            RunObservation::ThreadRegistered {
                source_sequence, ..
            } => Some(*source_sequence),
            RunObservation::InterruptAcknowledged { source_sequence }
            | RunObservation::ToolReserved {
                source_sequence, ..
            }
            | RunObservation::ToolStarted {
                source_sequence, ..
            }
            | RunObservation::ToolFinished {
                source_sequence, ..
            }
            | RunObservation::AdapterReported {
                source_sequence, ..
            }
            | RunObservation::AdapterGap { source_sequence } => Some(*source_sequence),
            _ => None,
        };
        if let Some(sequence) = sequence {
            if !matches!(self.state, RunState::Running | RunState::Stopping)
                || sequence <= self.source_cursor
            {
                return Err(invalid());
            }
            if self.source_cursor.checked_add(1) != Some(sequence) {
                self.event_gap = true;
            }
            self.source_cursor = sequence;
        }
        match observation {
            RunObservation::StartupConfigReported {
                request_id,
                payload_hash,
                ..
            } => {
                if self.state != RunState::Running
                    || self.source_thread.is_some()
                    || self.announced_thread.is_some()
                    || !request_id.valid()
                    || !crate::valid_evidence_hash(payload_hash)
                {
                    return Err(invalid());
                }
            }
            RunObservation::RemoteControlDisabled { payload_hash, .. } => {
                if self.state != RunState::Running || !crate::valid_evidence_hash(payload_hash) {
                    return Err(invalid());
                }
            }
            RunObservation::ThreadAnnounced {
                thread_id,
                payload_hash,
                ..
            } => {
                if self.state != RunState::Running
                    || self.announced_thread.is_some()
                    || !crate::valid_adapter_id(thread_id)
                    || !crate::valid_evidence_hash(payload_hash)
                    || self
                        .source_thread
                        .as_ref()
                        .is_some_and(|id| id != thread_id)
                {
                    return Err(invalid());
                }
                self.announced_thread = Some(thread_id.clone());
            }
            RunObservation::ThreadRegistered {
                effect_id,
                request_id,
                payload_hash,
                thread_id,
                ..
            } => {
                if self.state != RunState::Running
                    || self.source_thread.is_some()
                    || self
                        .announced_thread
                        .as_ref()
                        .is_some_and(|id| id != thread_id)
                    || !request_id.valid()
                    || !crate::valid_evidence_hash(payload_hash)
                    || !crate::valid_adapter_id(thread_id)
                    || !self.effects.iter().any(|effect| {
                        effect.effect_id == *effect_id
                            && effect.class == EffectClass::Unknown
                            && effect.state == EffectState::Started
                    })
                {
                    return Err(invalid());
                }
                self.source_thread = Some(thread_id.clone());
            }
            RunObservation::AdapterGap { .. } => self.event_gap = true,
            RunObservation::AdapterReported { event, .. } => {
                event.validate()?;
                if matches!(
                    event.event,
                    crate::AdapterEventKind::TurnInterruptPlanned { .. }
                ) {
                    self.state = RunState::Stopping;
                }
            }
            RunObservation::Spawned { pid } => {
                if self.state != RunState::LaunchPending || *pid == 0 {
                    return Err(invalid());
                }
                self.pid = Some(*pid);
                self.state = RunState::Running;
            }
            RunObservation::InterruptRequested => {
                if !matches!(self.state, RunState::Running | RunState::Stopping) {
                    return Err(invalid());
                }
                self.state = RunState::Stopping;
            }
            RunObservation::InterruptAcknowledged { .. } => {
                if self.state != RunState::Stopping {
                    return Err(invalid());
                }
            }
            RunObservation::ToolReserved {
                effect_id, class, ..
            }
            | RunObservation::ToolStarted {
                effect_id, class, ..
            } => {
                if matches!(observation, RunObservation::ToolStarted { .. }) {
                    if let Some(effect) =
                        self.effects.iter_mut().find(|e| e.effect_id == *effect_id)
                    {
                        if effect.state != EffectState::Reserved || effect.class != *class {
                            return Err(invalid());
                        }
                        effect.state = EffectState::Started;
                        return Ok(());
                    }
                }
                if effect_id.is_nil()
                    || self.effects.len() >= 4096
                    || self.effects.iter().any(|e| e.effect_id == *effect_id)
                {
                    return Err(invalid());
                }
                self.effects.push(EffectRecord {
                    effect_id: *effect_id,
                    class: *class,
                    state: if matches!(observation, RunObservation::ToolReserved { .. }) {
                        EffectState::Reserved
                    } else {
                        EffectState::Started
                    },
                    outcome: None,
                    reconciliation_hash: None,
                });
            }
            RunObservation::ToolFinished {
                effect_id, outcome, ..
            } => {
                let effect = self
                    .effects
                    .iter_mut()
                    .find(|e| e.effect_id == *effect_id)
                    .ok_or_else(invalid)?;
                if effect.state != EffectState::Started {
                    return Err(invalid());
                }
                effect.outcome = Some(*outcome);
                effect.state = if matches!(
                    effect.class,
                    EffectClass::ReadOnly | EffectClass::LocalReversible
                ) && *outcome != ToolOutcome::Interrupted
                {
                    EffectState::Completed
                } else {
                    EffectState::Uncertain
                };
            }
            RunObservation::TreeStopped => {
                if !matches!(self.state, RunState::Running | RunState::Stopping) {
                    return Err(invalid());
                }
                self.tree_stopped = true;
                self.state = RunState::Stopped;
                for effect in &mut self.effects {
                    if matches!(effect.state, EffectState::Started | EffectState::Reserved) {
                        effect.state = EffectState::Uncertain;
                    }
                }
            }
            RunObservation::SupervisorLost => {
                if self.state == RunState::Stopped {
                    return Err(invalid());
                }
                self.state = RunState::RecoveryRequired;
                self.event_gap = true;
                self.tree_stopped = false;
                for effect in &mut self.effects {
                    if matches!(effect.state, EffectState::Started | EffectState::Reserved) {
                        effect.state = EffectState::Uncertain;
                    }
                }
            }
            RunObservation::EffectReconciled {
                effect_id,
                evidence_hash,
            } => {
                if !valid_evidence_hash(evidence_hash) {
                    return Err(invalid());
                }
                let effect = self
                    .effects
                    .iter_mut()
                    .find(|e| e.effect_id == *effect_id)
                    .ok_or_else(invalid)?;
                if effect.state != EffectState::Uncertain {
                    return Err(invalid());
                }
                // shortcut: no resource-specific reconciler exists yet; a caller's
                // hash alone cannot establish that a remote mutation is settled.
                return Err(Diagnostic::new(ErrorCode::Blocked));
            }
        }
        Ok(())
    }

    /// Necessary checks only; absence of blockers is not an activation grant.
    pub fn blockers(&self) -> Vec<BoundaryReason> {
        // Capability declarations are not installed-runtime/event-coverage proof.
        let mut reasons = vec![BoundaryReason::AdapterUnvalidated];
        if self.state == RunState::LaunchPending {
            reasons.push(BoundaryReason::LaunchAmbiguous);
        }
        if self.state == RunState::RecoveryRequired {
            reasons.push(BoundaryReason::SupervisorLost);
        }
        if !self.tree_stopped {
            reasons.push(BoundaryReason::ProcessTreeUnconfirmed);
        }
        if self.event_gap {
            reasons.push(BoundaryReason::EventGap);
        }
        if self.effects.iter().any(|e| {
            matches!(
                e.state,
                EffectState::Reserved | EffectState::Started | EffectState::Uncertain
            )
        }) {
            reasons.push(BoundaryReason::UnresolvedEffect);
        }
        if !self.capabilities.lifecycle
            || !self.capabilities.tool_outcomes
            || !self.capabilities.interrupt
        {
            reasons.push(BoundaryReason::AdapterUnsupported);
        }
        if !self.capabilities.mutation_gating {
            reasons.push(BoundaryReason::MutationGatingUnavailable);
        }
        reasons
    }
}

pub fn valid_evidence_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionEvent {
    pub schema_version: SchemaVersion,
    pub event_id: Id,
    pub binding: RunBinding,
    pub sequence: u64,
    pub task_sequence: u64,
    pub operation_id: Id,
    pub timestamp_ms: i64,
    pub origin: ExecutionOrigin,
    pub state_hash: String,
    pub evidence_hash: Option<String>,
    pub observation: RunObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionOrigin {
    Supervisor,
    AdapterObserved,
    HostReported,
    ReconcilerReported,
}

impl RunObservation {
    pub fn origin(&self) -> ExecutionOrigin {
        match self {
            Self::ToolStarted { .. }
            | Self::StartupConfigReported { .. }
            | Self::ThreadRegistered { .. }
            | Self::ThreadAnnounced { .. }
            | Self::RemoteControlDisabled { .. }
            | Self::ToolFinished { .. }
            | Self::InterruptAcknowledged { .. } => ExecutionOrigin::AdapterObserved,
            Self::AdapterReported { event, .. } => match event.event {
                crate::AdapterEventKind::CallbackRefusal { phase, .. }
                    if phase != crate::CallbackPhase::Resolved =>
                {
                    ExecutionOrigin::HostReported
                }
                crate::AdapterEventKind::DynamicToolExecution(ref callback)
                    if callback.phase != crate::CallbackPhase::Resolved =>
                {
                    ExecutionOrigin::HostReported
                }
                crate::AdapterEventKind::ApprovalCancellationPlanned { .. }
                | crate::AdapterEventKind::ApprovalCancellationWritten { .. }
                | crate::AdapterEventKind::TurnInterruptPlanned { .. }
                | crate::AdapterEventKind::TurnInterruptWritten { .. } => {
                    ExecutionOrigin::HostReported
                }
                _ => ExecutionOrigin::AdapterObserved,
            },
            Self::AdapterGap { .. } => ExecutionOrigin::AdapterObserved,
            Self::EffectReconciled { .. } => ExecutionOrigin::ReconcilerReported,
            _ => ExecutionOrigin::Supervisor,
        }
    }
}
