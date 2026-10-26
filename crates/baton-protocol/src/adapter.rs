//! Bounded adapter observations; vendor identifiers never grant native control.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdapterToolKind {
    CommandExecution,
    FileChange,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PassiveItemKind {
    ClientToolCall,
    UserMessage,
    AgentMessage,
    Reasoning,
    Plan,
    ContextCompaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CallbackKind {
    UserInput,
    DynamicTool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CallbackPhase {
    Planned,
    Written,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AdapterRequestId {
    Number(i64),
    Text(String),
}
impl AdapterRequestId {
    pub fn valid(&self) -> bool {
        match self {
            Self::Number(_) => true,
            Self::Text(text) => valid_adapter_id(text),
        }
    }
    pub fn key(&self) -> String {
        match self {
            Self::Number(number) => format!("n:{number}"),
            Self::Text(text) => format!("s:{text}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionCallback {
    pub request_id: AdapterRequestId,
    pub correlation_id: String,
    pub payload_hash: String,
    pub phase: CallbackPhase,
    pub result_hash: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum AdapterEventKind {
    DynamicToolExecution(Box<ToolExecutionCallback>),
    CallbackRefusal {
        request_id: AdapterRequestId,
        correlation_id: String,
        callback: CallbackKind,
        payload_hash: String,
        phase: CallbackPhase,
    },
    TurnInterruptPlanned {
        request_id: AdapterRequestId,
    },
    TurnInterruptWritten {
        request_id: AdapterRequestId,
    },
    TurnInterruptAcknowledged {
        request_id: AdapterRequestId,
    },
    PassiveStarted {
        item_id: String,
        item: PassiveItemKind,
    },
    PassiveCompleted {
        item_id: String,
        item: PassiveItemKind,
    },
    ApprovalCancellationPlanned {
        request_id: AdapterRequestId,
        item_id: String,
        tool: AdapterToolKind,
    },
    ApprovalCancellationWritten {
        request_id: AdapterRequestId,
        item_id: String,
        tool: AdapterToolKind,
    },
    ApprovalResolutionReported {
        request_id: AdapterRequestId,
    },
    TurnStarted,
    TurnCompleted {
        outcome: ToolOutcome,
    },
    ToolStarted {
        item_id: String,
        effect_id: Id,
        tool: AdapterToolKind,
    },
    ToolCompleted {
        item_id: String,
        effect_id: Id,
        tool: AdapterToolKind,
        outcome: ToolOutcome,
        exit_code: Option<i32>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterEvent {
    pub adapter: AgentMetadata,
    pub thread_id: String,
    pub turn_id: String,
    pub event: AdapterEventKind,
}
impl AdapterEventKind {
    fn server_request(&self) -> Option<&AdapterRequestId> {
        match self {
            Self::DynamicToolExecution(callback) => Some(&callback.request_id),
            Self::CallbackRefusal { request_id, .. } => Some(request_id),
            _ => self.approval(),
        }
    }
    fn interrupt(&self) -> Option<&AdapterRequestId> {
        match self {
            Self::TurnInterruptPlanned { request_id }
            | Self::TurnInterruptWritten { request_id }
            | Self::TurnInterruptAcknowledged { request_id } => Some(request_id),
            _ => None,
        }
    }
    fn item(&self) -> Option<(&str, bool)> {
        match self {
            Self::ToolStarted { item_id, .. } | Self::PassiveStarted { item_id, .. } => {
                Some((item_id, true))
            }
            Self::ToolCompleted { item_id, .. } | Self::PassiveCompleted { item_id, .. } => {
                Some((item_id, false))
            }
            _ => None,
        }
    }
    fn same_item_shape(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::ToolStarted {
                    effect_id: a,
                    tool: at,
                    ..
                }
                | Self::ToolCompleted {
                    effect_id: a,
                    tool: at,
                    ..
                },
                Self::ToolStarted {
                    effect_id: b,
                    tool: bt,
                    ..
                }
                | Self::ToolCompleted {
                    effect_id: b,
                    tool: bt,
                    ..
                },
            ) => a == b && at == bt,
            (
                Self::PassiveStarted { item: a, .. } | Self::PassiveCompleted { item: a, .. },
                Self::PassiveStarted { item: b, .. } | Self::PassiveCompleted { item: b, .. },
            ) => a == b,
            _ => false,
        }
    }
    fn approval(&self) -> Option<&AdapterRequestId> {
        match self {
            Self::ApprovalCancellationPlanned { request_id, .. }
            | Self::ApprovalCancellationWritten { request_id, .. }
            | Self::ApprovalResolutionReported { request_id } => Some(request_id),
            _ => None,
        }
    }
}
pub fn valid_adapter_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}
impl AdapterEvent {
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if let AdapterEventKind::DynamicToolExecution(callback) = &self.event {
            let ToolExecutionCallback {
                request_id,
                correlation_id,
                payload_hash,
                phase,
                result_hash,
            } = callback.as_ref();
            if !request_id.valid()
                || !valid_adapter_id(correlation_id)
                || !valid_evidence_hash(payload_hash)
                || match phase {
                    CallbackPhase::Planned => result_hash.is_some(),
                    _ => result_hash
                        .as_ref()
                        .is_none_or(|hash| !valid_evidence_hash(hash)),
                }
            {
                return Err(Diagnostic::new(ErrorCode::InvalidInput));
            }
        }
        if let AdapterEventKind::CallbackRefusal {
            request_id,
            correlation_id,
            callback,
            payload_hash,
            phase,
        } = &self.event
        {
            if !request_id.valid()
                || !valid_adapter_id(correlation_id)
                || !valid_evidence_hash(payload_hash)
                || (*callback == CallbackKind::UserInput && *phase != CallbackPhase::Planned)
            {
                return Err(Diagnostic::new(ErrorCode::InvalidInput));
            }
        }
        if ![
            &self.adapter.name,
            &self.adapter.version,
            &self.thread_id,
            &self.turn_id,
        ]
        .into_iter()
        .all(|text| valid_adapter_id(text))
        {
            return Err(Diagnostic::new(ErrorCode::InvalidInput));
        }
        if self
            .event
            .item()
            .is_some_and(|(id, _)| !valid_adapter_id(id))
            || self.event.approval().is_some_and(|id| !id.valid())
            || self.event.interrupt().is_some_and(|id| !id.valid())
        {
            return Err(Diagnostic::new(ErrorCode::InvalidInput));
        }
        if let AdapterEventKind::ApprovalCancellationPlanned { item_id, .. }
        | AdapterEventKind::ApprovalCancellationWritten { item_id, .. } = &self.event
        {
            if !valid_adapter_id(item_id) {
                return Err(Diagnostic::new(ErrorCode::InvalidInput));
            }
        }
        if let AdapterEventKind::ToolStarted {
            item_id, effect_id, ..
        }
        | AdapterEventKind::ToolCompleted {
            item_id, effect_id, ..
        } = &self.event
        {
            if !valid_adapter_id(item_id) || effect_id.is_nil() {
                return Err(Diagnostic::new(ErrorCode::InvalidInput));
            }
        }
        if matches!(
            &self.event,
            AdapterEventKind::ToolCompleted {
                tool: AdapterToolKind::FileChange,
                exit_code: Some(_),
                ..
            }
        ) {
            return Err(Diagnostic::new(ErrorCode::InvalidInput));
        }
        if matches!(&self.event, AdapterEventKind::ToolCompleted {
            tool: AdapterToolKind::CommandExecution, outcome: ToolOutcome::Succeeded, exit_code, ..
        } if *exit_code != Some(0))
        {
            return Err(Diagnostic::new(ErrorCode::InvalidInput));
        }
        Ok(())
    }
    /// Missing predecessors are gaps; contradictory identities are refused.
    pub fn follows(&self, history: &[AdapterEvent]) -> Result<bool, Diagnostic> {
        self.validate()?;
        let invalid = || Diagnostic::new(ErrorCode::InvalidInput);
        if history
            .iter()
            .any(|old| old.adapter != self.adapter || old.thread_id != self.thread_id)
        {
            return Err(Diagnostic::new(ErrorCode::Forbidden));
        }
        let mut active = None;
        for old in history {
            match old.event {
                AdapterEventKind::TurnStarted => active = Some(old.turn_id.as_str()),
                AdapterEventKind::TurnCompleted { .. } if active == Some(old.turn_id.as_str()) => {
                    active = None
                }
                _ => (),
            }
        }
        let mut gap = active != Some(self.turn_id.as_str());
        for old in history {
            if self.event.server_request().is_some()
                && self.event.server_request() == old.event.server_request()
                && matches!(self.event, AdapterEventKind::DynamicToolExecution(_))
                    != matches!(old.event, AdapterEventKind::DynamicToolExecution(_))
            {
                return Err(invalid());
            }
        }
        if let AdapterEventKind::DynamicToolExecution(callback) = &self.event {
            let ToolExecutionCallback {
                request_id,
                correlation_id,
                payload_hash,
                phase,
                result_hash,
            } = callback.as_ref();
            let mut planned = false;
            let mut written = false;
            for old in history {
                if let AdapterEventKind::DynamicToolExecution(old_callback) = &old.event {
                    let ToolExecutionCallback {
                        request_id: id,
                        correlation_id: old_id,
                        payload_hash: old_hash,
                        phase: old_phase,
                        result_hash: old_result,
                    } = old_callback.as_ref();
                    if id != request_id {
                        continue;
                    }
                    if old.turn_id != self.turn_id
                        || old_id != correlation_id
                        || old_hash != payload_hash
                        || old_phase == phase
                        || *old_phase == CallbackPhase::Resolved
                        || (old_result.is_some() && old_result != result_hash)
                    {
                        return Err(invalid());
                    }
                    planned |= *old_phase == CallbackPhase::Planned;
                    written |= *old_phase == CallbackPhase::Written;
                }
            }
            if *phase != CallbackPhase::Planned && !planned {
                return Err(invalid());
            }
            if *phase == CallbackPhase::Resolved {
                gap = !written;
            }
        }
        if let AdapterEventKind::CallbackRefusal {
            request_id,
            correlation_id,
            callback,
            payload_hash,
            phase,
        } = &self.event
        {
            let mut planned = false;
            let mut written = false;
            for old in history {
                if old.event.approval() == Some(request_id) {
                    return Err(invalid());
                }
                if let AdapterEventKind::CallbackRefusal {
                    request_id: id,
                    correlation_id: old_id,
                    callback: old_kind,
                    payload_hash: old_hash,
                    phase: old_phase,
                } = &old.event
                {
                    if id != request_id {
                        continue;
                    }
                    if old.turn_id != self.turn_id
                        || old_id != correlation_id
                        || old_kind != callback
                        || old_hash != payload_hash
                        || old_phase == phase
                        || *old_phase == CallbackPhase::Resolved
                    {
                        return Err(invalid());
                    }
                    planned |= *old_phase == CallbackPhase::Planned;
                    written |= *old_phase == CallbackPhase::Written;
                }
            }
            if *phase != CallbackPhase::Planned && !planned {
                return Err(invalid());
            }
            if *phase == CallbackPhase::Resolved {
                gap = !written;
            }
        }
        if let Some(request_id) = self.event.interrupt() {
            let mut planned = false;
            let mut written = false;
            for old in history
                .iter()
                .filter(|old| old.event.interrupt() == Some(request_id))
            {
                if old.turn_id != self.turn_id
                    || std::mem::discriminant(&old.event) == std::mem::discriminant(&self.event)
                {
                    return Err(invalid());
                }
                planned |= matches!(old.event, AdapterEventKind::TurnInterruptPlanned { .. });
                written |= matches!(old.event, AdapterEventKind::TurnInterruptWritten { .. });
            }
            match self.event {
                AdapterEventKind::TurnInterruptPlanned { .. } if gap => return Err(invalid()),
                AdapterEventKind::TurnInterruptWritten { .. } if !planned => return Err(invalid()),
                AdapterEventKind::TurnInterruptAcknowledged { .. } if !planned || !written => {
                    return Err(invalid())
                }
                _ => (),
            }
            // The correlated reply may arrive after the terminal turn notification.
            gap = false;
        }
        if let Some(request_id) = self.event.approval() {
            if history.iter().any(|old| matches!(&old.event, AdapterEventKind::CallbackRefusal { request_id: id, .. } if id == request_id)) { return Err(invalid()); }
            let mut planned = false;
            let mut written = false;
            for old in history
                .iter()
                .filter(|old| old.event.approval() == Some(request_id))
            {
                if old.turn_id != self.turn_id {
                    return Err(invalid());
                }
                match (&old.event, &self.event) {
                    (
                        AdapterEventKind::ApprovalCancellationPlanned { .. },
                        AdapterEventKind::ApprovalCancellationPlanned { .. },
                    )
                    | (
                        AdapterEventKind::ApprovalCancellationWritten { .. },
                        AdapterEventKind::ApprovalCancellationWritten { .. },
                    )
                    | (
                        AdapterEventKind::ApprovalResolutionReported { .. },
                        AdapterEventKind::ApprovalResolutionReported { .. },
                    ) => return Err(invalid()),
                    _ => (),
                }
                if let AdapterEventKind::ApprovalCancellationPlanned {
                    item_id: a,
                    tool: at,
                    ..
                }
                | AdapterEventKind::ApprovalCancellationWritten {
                    item_id: a,
                    tool: at,
                    ..
                } = &old.event
                {
                    if let AdapterEventKind::ApprovalCancellationPlanned {
                        item_id: b,
                        tool: bt,
                        ..
                    }
                    | AdapterEventKind::ApprovalCancellationWritten {
                        item_id: b,
                        tool: bt,
                        ..
                    } = &self.event
                    {
                        if a != b || at != bt {
                            return Err(invalid());
                        }
                    }
                }
                planned |= matches!(
                    old.event,
                    AdapterEventKind::ApprovalCancellationPlanned { .. }
                );
                written |= matches!(
                    old.event,
                    AdapterEventKind::ApprovalCancellationWritten { .. }
                );
            }
            if !matches!(
                self.event,
                AdapterEventKind::ApprovalCancellationPlanned { .. }
            ) && !planned
            {
                return Err(invalid());
            }
            if matches!(
                self.event,
                AdapterEventKind::ApprovalResolutionReported { .. }
            ) && !written
            {
                gap = true;
            }
        }
        for old in history.iter().filter(|old| old.turn_id == self.turn_id) {
            match (&old.event, &self.event) {
                (AdapterEventKind::TurnStarted, AdapterEventKind::TurnStarted)
                | (
                    AdapterEventKind::TurnCompleted { .. },
                    AdapterEventKind::TurnCompleted { .. },
                ) => return Err(invalid()),
                _ => (),
            }
            if let (Some((a, a_started)), Some((b, b_started))) =
                (old.event.item(), self.event.item())
            {
                if a == b && (a_started == b_started || !old.event.same_item_shape(&self.event)) {
                    return Err(invalid());
                }
            }
        }
        match &self.event {
            AdapterEventKind::TurnStarted => gap = active.is_some(),
            AdapterEventKind::ToolCompleted { item_id, .. } => {
                gap |= !history.iter().any(|old| old.turn_id == self.turn_id
                    && matches!(&old.event, AdapterEventKind::ToolStarted { item_id:id,.. } if id == item_id));
            }
            AdapterEventKind::PassiveCompleted { item_id, .. } => {
                gap |= !history.iter().any(|old|old.turn_id==self.turn_id
                    && matches!(&old.event,AdapterEventKind::PassiveStarted{item_id:id,..} if id==item_id));
            }
            AdapterEventKind::ApprovalCancellationPlanned { item_id, tool, .. } => {
                gap |= !history.iter().any(|old|old.turn_id==self.turn_id
                    && matches!(&old.event,AdapterEventKind::ToolStarted{item_id:id,tool:kind,..} if id==item_id && kind==tool));
            }
            _ => (),
        }
        Ok(gap)
    }
}
