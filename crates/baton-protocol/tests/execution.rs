use baton_protocol::*;

fn run() -> RunRecord {
    RunRecord::pending(
        RunBinding {
            run_id: Id::new_v4(),
            task_id: Id::new_v4(),
            actor_id: Id::new_v4(),
            lease_id: Id::new_v4(),
            epoch: 1,
        },
        "0".repeat(64),
        AdapterCapabilities {
            lifecycle: true,
            tool_outcomes: true,
            interrupt: true,
            mutation_gating: false,
        },
    )
}

// M3-P01; FR-03/06/15, INV-09: interruption is not process drain.
#[test]
fn m3_interrupt_ack_and_parent_exit_never_establish_quiescence() {
    let mut r = run();
    r.apply(&RunObservation::Spawned { pid: 42 }).unwrap();
    r.apply(&RunObservation::InterruptRequested).unwrap();
    r.apply(&RunObservation::InterruptAcknowledged { source_sequence: 1 })
        .unwrap();
    assert_eq!(r.state, RunState::Stopping);
    assert!(!r.tree_stopped);
    assert!(r
        .blockers()
        .contains(&BoundaryReason::ProcessTreeUnconfirmed));
    r.apply(&RunObservation::TreeStopped).unwrap();
    assert!(!r
        .blockers()
        .contains(&BoundaryReason::ProcessTreeUnconfirmed));
    assert!(r
        .blockers()
        .contains(&BoundaryReason::MutationGatingUnavailable));
}

// T-007-M3; FR-14, INV-04: external exit success cannot reconcile the service.
#[test]
fn m3_external_effect_requires_explicit_evidence_and_never_replays() {
    let mut r = run();
    let id = Id::new_v4();
    r.apply(&RunObservation::Spawned { pid: 42 }).unwrap();
    r.apply(&RunObservation::ToolStarted {
        source_sequence: 1,
        effect_id: id,
        class: EffectClass::ExternalNonIdempotent,
    })
    .unwrap();
    r.apply(&RunObservation::ToolFinished {
        source_sequence: 2,
        effect_id: id,
        outcome: ToolOutcome::Succeeded,
    })
    .unwrap();
    r.apply(&RunObservation::TreeStopped).unwrap();
    assert_eq!(r.effects[0].state, EffectState::Uncertain);
    assert!(r.blockers().contains(&BoundaryReason::UnresolvedEffect));
    assert!(r
        .apply(&RunObservation::EffectReconciled {
            effect_id: id,
            evidence_hash: "bad".into()
        })
        .is_err());
    assert_eq!(
        r.apply(&RunObservation::EffectReconciled {
            effect_id: id,
            evidence_hash: "a".repeat(64)
        })
        .unwrap_err()
        .code,
        ErrorCode::Blocked
    );
    assert!(r.blockers().contains(&BoundaryReason::UnresolvedEffect));
}

// M3-P02; FR-03/12, INV-09: gaps and lost process identity remain sticky.
#[test]
fn m3_gap_loss_and_strict_schema() {
    let mut r = run();
    r.apply(&RunObservation::Spawned { pid: 42 }).unwrap();
    r.apply(&RunObservation::InterruptRequested).unwrap();
    r.apply(&RunObservation::InterruptAcknowledged { source_sequence: 3 })
        .unwrap();
    assert!(r.event_gap);
    r.apply(&RunObservation::SupervisorLost).unwrap();
    assert_eq!(r.state, RunState::RecoveryRequired);
    assert!(r.apply(&RunObservation::TreeStopped).is_err());
    assert!(r.blockers().contains(&BoundaryReason::SupervisorLost));
    let mut json = serde_json::to_value(&r).unwrap();
    json["schema_version"] = "0.2.0".into();
    assert!(serde_json::from_value::<RunRecord>(json).is_err());
    let mut json = serde_json::to_value(&r).unwrap();
    json["hidden_state"] = true.into();
    assert!(serde_json::from_value::<RunRecord>(json).is_err());
}

// M3-P03; FR-15, INV-09: declared flags are not installed-runtime evidence.
#[test]
fn m3_capability_claims_cannot_certify_event_coverage() {
    let mut r = run();
    r.capabilities.mutation_gating = true;
    r.apply(&RunObservation::Spawned { pid: 42 }).unwrap();
    r.apply(&RunObservation::TreeStopped).unwrap();
    assert!(r.blockers().contains(&BoundaryReason::AdapterUnvalidated));
}
