use baton_protocol::*;
fn run() -> RunRecord {
    let mut run = RunRecord::pending(
        RunBinding {
            task_id: Id::new_v4(),
            run_id: Id::new_v4(),
            actor_id: Id::new_v4(),
            lease_id: Id::new_v4(),
            epoch: 1,
        },
        "0".repeat(64),
        AdapterCapabilities {
            lifecycle: false,
            tool_outcomes: false,
            interrupt: false,
            mutation_gating: false,
        },
    );
    run.apply(&RunObservation::Spawned { pid: 1 }).unwrap();
    run
}
fn event(event: AdapterEventKind) -> AdapterEvent {
    AdapterEvent {
        adapter: AgentMetadata {
            name: "codex".into(),
            version: "0.164.0".into(),
        },
        thread_id: "thread-1".into(),
        turn_id: "turn-1".into(),
        event,
    }
}
// M5-E01; FR-03/14/15, INV-04/09: vendor success cannot settle an unknown command.
#[test]
fn adapter_tool_completion_keeps_unknown_effect_and_origin() {
    let mut run = run();
    let effect_id = Id::new_v4();
    let start = RunObservation::AdapterReported {
        source_sequence: 1,
        event: event(AdapterEventKind::ToolStarted {
            item_id: "item-1".into(),
            effect_id,
            tool: AdapterToolKind::CommandExecution,
        }),
    };
    assert_eq!(start.origin(), ExecutionOrigin::AdapterObserved);
    run.apply(&start).unwrap();
    assert_eq!(run.effects.len(), 1);
    run.apply(&RunObservation::AdapterReported {
        source_sequence: 2,
        event: event(AdapterEventKind::ToolCompleted {
            item_id: "item-1".into(),
            effect_id,
            tool: AdapterToolKind::CommandExecution,
            outcome: ToolOutcome::Succeeded,
            exit_code: Some(0),
        }),
    })
    .unwrap();
    assert_eq!(run.effects[0].state, EffectState::Uncertain);
    assert_eq!(run.effects[0].outcome, Some(ToolOutcome::Succeeded));
    assert!(!run.tree_stopped);
}

// M5-E02; missing starts remain recorded as gaps and uncertain effects.
#[test]
fn orphan_completion_is_retained_without_inventing_a_safe_start() {
    let mut run = run();
    run.apply(&RunObservation::AdapterReported {
        source_sequence: 1,
        event: event(AdapterEventKind::ToolCompleted {
            item_id: "orphan".into(),
            effect_id: Id::new_v4(),
            tool: AdapterToolKind::FileChange,
            outcome: ToolOutcome::Failed,
            exit_code: None,
        }),
    })
    .unwrap();
    assert!(run.event_gap);
    assert_eq!(run.effects[0].state, EffectState::Uncertain);
    let before = run.clone();
    let mut invalid = event(AdapterEventKind::TurnStarted);
    invalid.thread_id = "../../bad\ntext".into();
    assert!(run
        .apply(&RunObservation::AdapterReported {
            source_sequence: 2,
            event: invalid
        })
        .is_err());
    assert_eq!(run, before);
}

// M5-E02; journal-derived turn correlation cannot silently rebind or erase a gap.
#[test]
fn stream_predecessors_duplicates_and_rebinding_are_distinct() {
    let start = event(AdapterEventKind::TurnStarted);
    assert!(!start.follows(&[]).unwrap());
    assert!(start.follows(std::slice::from_ref(&start)).is_err());
    let done = event(AdapterEventKind::TurnCompleted {
        outcome: ToolOutcome::Succeeded,
    });
    assert!(done.follows(&[]).unwrap());
    assert!(!done.follows(std::slice::from_ref(&start)).unwrap());
    let mut other = start.clone();
    other.thread_id = "another-thread".into();
    assert_eq!(
        other.follows(&[start]).unwrap_err().code,
        ErrorCode::Forbidden
    );
    let mut run = run();
    run.apply(&RunObservation::SupervisorLost).unwrap();
    assert!(run.event_gap);
}
