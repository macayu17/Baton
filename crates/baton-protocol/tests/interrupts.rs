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

// M5-I01; FR-03/06/12, INV-06/09: intent, reply, terminal turn and native drain differ.
#[test]
fn interrupt_requires_an_active_turn_and_never_proves_drain() {
    let request_id = AdapterRequestId::Text("interrupt-1".into());
    let planned = event(AdapterEventKind::TurnInterruptPlanned {
        request_id: request_id.clone(),
    });
    let written = event(AdapterEventKind::TurnInterruptWritten {
        request_id: request_id.clone(),
    });
    let acknowledged = event(AdapterEventKind::TurnInterruptAcknowledged { request_id });
    assert!(planned.follows(&[]).is_err());
    let started = event(AdapterEventKind::TurnStarted);
    assert!(!planned.follows(std::slice::from_ref(&started)).unwrap());
    assert!(acknowledged
        .follows(std::slice::from_ref(&started))
        .is_err());
    assert!(!written
        .follows(&[started.clone(), planned.clone()])
        .unwrap());
    let completed = event(AdapterEventKind::TurnCompleted {
        outcome: ToolOutcome::Interrupted,
    });
    assert!(!acknowledged
        .follows(&[started, planned.clone(), written.clone(), completed])
        .unwrap());
    for control in [planned, written] {
        assert_eq!(
            RunObservation::AdapterReported {
                source_sequence: 1,
                event: control
            }
            .origin(),
            ExecutionOrigin::HostReported
        );
    }
    assert_eq!(
        RunObservation::AdapterReported {
            source_sequence: 1,
            event: acknowledged
        }
        .origin(),
        ExecutionOrigin::AdapterObserved
    );
}
