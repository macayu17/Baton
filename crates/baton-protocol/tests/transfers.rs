use baton_protocol::*;

fn record() -> TransferRecord {
    TransferRecord::requested(TransferBinding {
        transfer_id: Id::new_v4(),
        task_id: Id::new_v4(),
        source_id: Id::new_v4(),
        receiver_id: Id::new_v4(),
        source_epoch: 1,
    })
    .unwrap()
}
fn facts() -> TransferFacts {
    TransferFacts {
        source_fenced: true,
        events_complete: true,
        effects_settled: true,
        adapter_validated: true,
        artifacts_verified: true,
        artifacts_durable: true,
        source_isolated: true,
        receiver_verified: true,
        receiver_started: true,
        optional_context_missing: false,
    }
}

// FR-13, INV-08: context loss discovered after commit needs reconciliation.
#[test]
fn m4_transfer_model_new_postcommit_context_loss_cannot_silently_activate() {
    let mut transfer = record();
    let b = transfer.binding.clone();
    transfer
        .advance(
            b.source_id,
            1,
            TransferStep::Prepare(checkpoint()),
            &facts(),
        )
        .unwrap();
    transfer
        .advance(b.receiver_id, 1, TransferStep::BeginRestore, &facts())
        .unwrap();
    transfer
        .advance(
            b.receiver_id,
            1,
            TransferStep::ReceiverValidated {
                restore_id: Id::new_v4(),
            },
            &facts(),
        )
        .unwrap();
    transfer
        .advance(
            b.receiver_id,
            1,
            TransferStep::Commit {
                accept_degraded: false,
            },
            &facts(),
        )
        .unwrap();
    let mut lost = facts();
    lost.optional_context_missing = true;
    transfer
        .advance(b.receiver_id, 2, TransferStep::Activate, &lost)
        .unwrap();
    assert_eq!(transfer.state, TransferState::RecoveryRequired);
    assert_eq!(transfer.quality, RecoveryQuality::Blocked);
    assert_eq!(transfer.committed_epoch, Some(2));
}
fn checkpoint() -> TransferCheckpoint {
    TransferCheckpoint {
        checkpoint_id: Id::new_v4(),
        manifest_hash: "a".repeat(64),
        event_cursor: 2,
    }
}

// T-003-M4 model; FR-08/09, INV-02: no receiver activation before all prior stages.
#[test]
fn m4_transfer_model_orders_restore_commit_and_activation() {
    let mut transfer = record();
    let binding = transfer.binding.clone();
    assert!(transfer
        .advance(binding.receiver_id, 1, TransferStep::Activate, &facts())
        .is_err());
    transfer
        .advance(
            binding.source_id,
            1,
            TransferStep::Prepare(checkpoint()),
            &facts(),
        )
        .unwrap();
    transfer
        .advance(binding.receiver_id, 1, TransferStep::BeginRestore, &facts())
        .unwrap();
    transfer
        .advance(
            binding.receiver_id,
            1,
            TransferStep::ReceiverValidated {
                restore_id: Id::new_v4(),
            },
            &facts(),
        )
        .unwrap();
    assert!(transfer
        .advance(
            binding.source_id,
            1,
            TransferStep::Commit {
                accept_degraded: false
            },
            &facts()
        )
        .is_err());
    transfer
        .advance(
            binding.receiver_id,
            1,
            TransferStep::Commit {
                accept_degraded: false,
            },
            &facts(),
        )
        .unwrap();
    assert_eq!(transfer.committed_epoch, Some(2));
    assert_eq!(transfer.state, TransferState::Committed);
    assert!(transfer
        .advance(binding.receiver_id, 1, TransferStep::Activate, &facts())
        .is_err());
    transfer
        .advance(binding.receiver_id, 2, TransferStep::Activate, &facts())
        .unwrap();
    assert_eq!(transfer.state, TransferState::Activating);
    transfer
        .advance(
            binding.receiver_id,
            2,
            TransferStep::ReceiverStarted,
            &facts(),
        )
        .unwrap();
    assert_eq!(transfer.state, TransferState::Active);
    assert_eq!(transfer.quality, RecoveryQuality::Verified);
    assert!(transfer
        .advance(
            binding.receiver_id,
            2,
            TransferStep::Commit {
                accept_degraded: false
            },
            &facts()
        )
        .is_err());
}

// T-009-M4 model; FR-13, INV-04/06/08/09: acknowledgement cannot override unsafe facts.
#[test]
fn m4_transfer_model_blocks_unsafe_effects_and_requires_degraded_ack() {
    let mut transfer = record();
    let mut unsafe_facts = facts();
    unsafe_facts.effects_settled = false;
    transfer
        .advance(
            transfer.binding.source_id,
            1,
            TransferStep::Prepare(checkpoint()),
            &unsafe_facts,
        )
        .unwrap();
    assert_eq!(transfer.state, TransferState::Blocked);
    assert!(transfer
        .reasons
        .contains(&TransferReason::UnresolvedEffects));
    assert!(transfer
        .advance(
            transfer.binding.receiver_id,
            1,
            TransferStep::Commit {
                accept_degraded: true
            },
            &unsafe_facts
        )
        .is_err());

    let mut transfer = record();
    let mut missing = facts();
    missing.optional_context_missing = true;
    transfer
        .advance(
            transfer.binding.source_id,
            1,
            TransferStep::Prepare(checkpoint()),
            &missing,
        )
        .unwrap();
    transfer
        .advance(
            transfer.binding.receiver_id,
            1,
            TransferStep::BeginRestore,
            &missing,
        )
        .unwrap();
    transfer
        .advance(
            transfer.binding.receiver_id,
            1,
            TransferStep::ReceiverValidated {
                restore_id: Id::new_v4(),
            },
            &missing,
        )
        .unwrap();
    transfer
        .advance(
            transfer.binding.receiver_id,
            1,
            TransferStep::Commit {
                accept_degraded: false,
            },
            &missing,
        )
        .unwrap();
    assert_eq!(transfer.state, TransferState::ReadyToCommit);
    assert_eq!(transfer.committed_epoch, None);
    transfer
        .advance(
            transfer.binding.receiver_id,
            1,
            TransferStep::Commit {
                accept_degraded: true,
            },
            &missing,
        )
        .unwrap();
    assert_eq!(transfer.committed_epoch, Some(2));
}

// M4-P03; FR-12, INV-01/07: recovery never rolls a committed epoch back.
#[test]
fn m4_committed_transfer_recovery_preserves_receiver_epoch_and_strict_schema() {
    let mut transfer = record();
    transfer
        .advance(
            transfer.binding.source_id,
            1,
            TransferStep::Prepare(checkpoint()),
            &facts(),
        )
        .unwrap();
    transfer
        .advance(
            transfer.binding.receiver_id,
            1,
            TransferStep::BeginRestore,
            &facts(),
        )
        .unwrap();
    transfer
        .advance(
            transfer.binding.receiver_id,
            1,
            TransferStep::ReceiverValidated {
                restore_id: Id::new_v4(),
            },
            &facts(),
        )
        .unwrap();
    transfer
        .advance(
            transfer.binding.receiver_id,
            1,
            TransferStep::Commit {
                accept_degraded: false,
            },
            &facts(),
        )
        .unwrap();
    assert!(transfer
        .advance(transfer.binding.source_id, 1, TransferStep::Abort, &facts())
        .is_err());
    transfer
        .advance(
            transfer.binding.receiver_id,
            2,
            TransferStep::Recover,
            &facts(),
        )
        .unwrap();
    assert_eq!(transfer.committed_epoch, Some(2));
    assert_eq!(transfer.state, TransferState::RecoveryRequired);
    let mut json = serde_json::to_value(&transfer).unwrap();
    json["source_fenced"] = true.into();
    assert!(serde_json::from_value::<TransferRecord>(json).is_err());
}
