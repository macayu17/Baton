use baton_protocol::*;
fn event(kind: AdapterEventKind) -> AdapterEvent {
    AdapterEvent {
        adapter: AgentMetadata {
            name: "codex".into(),
            version: "0.164.0".into(),
        },
        thread_id: "thread-1".into(),
        turn_id: "turn-1".into(),
        event: kind,
    }
}
// M5-A01; FR-03/14/15, INV-04/09: host intent, written bytes and reported resolution are distinct.
#[test]
fn cancellation_requires_intent_and_has_host_provenance() {
    let request_id = AdapterRequestId::Number(7);
    let written = event(AdapterEventKind::ApprovalCancellationWritten {
        request_id: request_id.clone(),
        item_id: "item-1".into(),
        tool: AdapterToolKind::CommandExecution,
    });
    assert!(written.follows(&[]).is_err());
    let planned = event(AdapterEventKind::ApprovalCancellationPlanned {
        request_id: request_id.clone(),
        item_id: "item-1".into(),
        tool: AdapterToolKind::CommandExecution,
    });
    assert_eq!(
        RunObservation::AdapterReported {
            source_sequence: 1,
            event: planned.clone()
        }
        .origin(),
        ExecutionOrigin::HostReported
    );
    assert!(written.follows(&[planned]).is_ok());
    assert_ne!(request_id, AdapterRequestId::Text("7".into()));
}
// M5-P01; FR-03/15, INV-09: passive item metadata is not a tool effect or complete text capture.
#[test]
fn passive_completion_without_start_is_a_gap_and_ids_are_bounded() {
    let start = event(AdapterEventKind::TurnStarted);
    let mut completed = event(AdapterEventKind::PassiveCompleted {
        item_id: "item-1".into(),
        item: PassiveItemKind::AgentMessage,
    });
    assert!(completed.follows(&[start]).unwrap());
    completed.event = AdapterEventKind::PassiveCompleted {
        item_id: "../escape".into(),
        item: PassiveItemKind::Reasoning,
    };
    assert!(completed.validate().is_err());
}
