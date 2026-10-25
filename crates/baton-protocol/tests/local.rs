use baton_protocol::*;

// M4-P01; FR-12/13, INV-02/09: local clients cannot supply authority facts.
#[test]
fn local_envelope_is_strict_and_does_not_accept_an_actor_or_fencing_proof() {
    let request = LocalRequest {
        schema_version: SchemaVersion::V0_1_0,
        request_id: Id::new_v4(),
        expected_epoch: 0,
        command: LocalCommand::Status {
            task_id: Id::new_v4(),
        },
    };
    let json = serde_json::to_value(&request).unwrap();
    for field in ["actor_id", "source_fenced", "receiver_verified"] {
        let mut forged = json.clone();
        forged[field] = true.into();
        assert!(serde_json::from_value::<LocalRequest>(forged).is_err());
    }
    let action = Action::ObserveRun {
        task_id: Id::new_v4(),
        lease_id: Id::new_v4(),
        run_id: Id::new_v4(),
        observation: RunObservation::TreeStopped,
    };
    assert!(!local_mutation_allowed(&action));
    assert!(local_mutation_allowed(&Action::ClaimTask {
        task_id: Id::new_v4(),
        lease_ms: 100
    }));
}
