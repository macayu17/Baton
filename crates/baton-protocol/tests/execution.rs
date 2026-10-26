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

// M5-SP02; configuration testimony grants no thread, effect settlement or native boundary.
#[test]
fn startup_configuration_report_is_bounded_metadata_without_authority() {
    let mut r = run();
    r.apply(&RunObservation::Spawned { pid: 42 }).unwrap();
    let observation = RunObservation::StartupConfigReported {
        source_sequence: 1,
        request_id: AdapterRequestId::Number(7),
        payload_hash: "a".repeat(64),
        services_present: false,
    };
    r.apply(&observation).unwrap();
    assert_eq!(observation.origin(), ExecutionOrigin::AdapterObserved);
    assert!(r.source_thread.is_none());
    assert!(r.effects.is_empty());
    assert!(!r.tree_stopped);
    let before = r.clone();
    let mut invalid = observation;
    if let RunObservation::StartupConfigReported {
        source_sequence,
        payload_hash,
        ..
    } = &mut invalid
    {
        *source_sequence = 2;
        *payload_hash = "bad".into();
    }
    assert!(r.apply(&invalid).is_err());
    assert_eq!(r, before);
}

// M5-R01; FR-03/06/15, INV-07/09: registration requires an admitted started effect.
#[test]
fn thread_registration_requires_started_host_work_and_valid_identity() {
    let mut r = run();
    r.apply(&RunObservation::Spawned { pid: 42 }).unwrap();
    let effect = Id::new_v4();
    let observed = RunObservation::ThreadRegistered {
        source_sequence: 3,
        effect_id: effect,
        request_id: AdapterRequestId::Text("request-1".into()),
        payload_hash: "a".repeat(64),
        thread_id: "thread-1".into(),
    };
    assert!(r.apply(&observed).is_err());
    r.apply(&RunObservation::ToolReserved {
        source_sequence: 1,
        effect_id: effect,
        class: EffectClass::Unknown,
    })
    .unwrap();
    assert!(r.apply(&observed).is_err());
    r.apply(&RunObservation::ToolStarted {
        source_sequence: 2,
        effect_id: effect,
        class: EffectClass::Unknown,
    })
    .unwrap();
    r.apply(&observed).unwrap();
    assert_eq!(observed.origin(), ExecutionOrigin::AdapterObserved);
    assert_eq!(r.effects[0].state, EffectState::Started);
    assert!(!r.tree_stopped);
    let mut invalid = observed;
    if let RunObservation::ThreadRegistered {
        thread_id,
        source_sequence,
        ..
    } = &mut invalid
    {
        *thread_id = "../invalid".into();
        *source_sequence = 4;
    }
    assert!(r.apply(&invalid).is_err());
}

// M5-R01; notification and request acknowledgement are separate facts in either order.
#[test]
fn thread_announcement_cannot_rebind_an_acknowledged_thread() {
    let mut r = run();
    r.apply(&RunObservation::Spawned { pid: 42 }).unwrap();
    r.apply(&RunObservation::ThreadAnnounced {
        source_sequence: 1,
        thread_id: "thread-1".into(),
        payload_hash: "a".repeat(64),
    })
    .unwrap();
    assert_eq!(r.announced_thread.as_deref(), Some("thread-1"));
    assert!(r.source_thread.is_none());
    let effect = Id::new_v4();
    r.apply(&RunObservation::ToolStarted {
        source_sequence: 2,
        effect_id: effect,
        class: EffectClass::LocalReversible,
    })
    .unwrap();
    assert!(r
        .apply(&RunObservation::ThreadRegistered {
            source_sequence: 3,
            effect_id: effect,
            request_id: AdapterRequestId::Number(2),
            payload_hash: "b".repeat(64),
            thread_id: "other-thread".into()
        })
        .is_err());
}

// M5-R01; only an observed disabled remote-control status is supported.
#[test]
fn remote_control_disabled_is_metadata_without_effect_or_authority() {
    let mut r = run();
    r.apply(&RunObservation::Spawned { pid: 42 }).unwrap();
    let observation = RunObservation::RemoteControlDisabled {
        source_sequence: 1,
        payload_hash: "a".repeat(64),
    };
    r.apply(&observation).unwrap();
    assert_eq!(observation.origin(), ExecutionOrigin::AdapterObserved);
    assert!(r.effects.is_empty());
    assert!(!r.tree_stopped);
    assert!(r.source_thread.is_none());
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
