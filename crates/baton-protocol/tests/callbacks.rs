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
// M5-D01/U01; FR-03/06/14/15, INV-04/07/09: refusing a callback cannot authorize effects.
#[test]
fn callback_refusal_binds_payload_and_has_distinct_phases() {
    let planned = event(AdapterEventKind::CallbackRefusal {
        request_id: AdapterRequestId::Number(7),
        correlation_id: "call-1".into(),
        callback: CallbackKind::DynamicTool,
        payload_hash: "a".repeat(64),
        phase: CallbackPhase::Planned,
    });
    let mut written = planned.clone();
    if let AdapterEventKind::CallbackRefusal { phase, .. } = &mut written.event {
        *phase = CallbackPhase::Written;
    }
    assert!(written.follows(&[]).is_err());
    let started = event(AdapterEventKind::TurnStarted);
    assert!(!written.follows(&[started, planned.clone()]).unwrap());
    let mut changed = written.clone();
    if let AdapterEventKind::CallbackRefusal { payload_hash, .. } = &mut changed.event {
        *payload_hash = "b".repeat(64);
    }
    assert!(changed.follows(std::slice::from_ref(&planned)).is_err());
    assert_eq!(
        RunObservation::AdapterReported {
            source_sequence: 1,
            event: planned.clone()
        }
        .origin(),
        ExecutionOrigin::HostReported
    );
    let mut resolved = planned.clone();
    if let AdapterEventKind::CallbackRefusal { phase, .. } = &mut resolved.event {
        *phase = CallbackPhase::Resolved;
    }
    assert!(
        written.follows(&[planned.clone(), resolved]).is_err(),
        "written evidence cannot follow terminal resolution"
    );
    if let AdapterEventKind::CallbackRefusal { callback, .. } = &mut written.event {
        *callback = CallbackKind::UserInput;
    }
    assert!(
        written.validate().is_err(),
        "no fabricated input answer or written phase"
    );
    let approval = event(AdapterEventKind::ApprovalCancellationPlanned {
        request_id: AdapterRequestId::Number(7),
        item_id: "item-1".into(),
        tool: AdapterToolKind::CommandExecution,
    });
    assert!(planned.follows(std::slice::from_ref(&approval)).is_err());
    assert!(approval.follows(&[planned]).is_err());
}
