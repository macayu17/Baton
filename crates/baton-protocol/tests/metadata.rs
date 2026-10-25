use baton_protocol::*;

// M1-P01; FR-02/07/08, INV-01/07: unknown authority fields/versions must reject.
#[test]
fn m1_p01_requests_reject_unknown_fields_and_versions() {
    let req = Request {
        schema_version: SchemaVersion::V0_1_0,
        operation_id: Id::new_v4(),
        actor_id: Id::new_v4(),
        expected_epoch: 0,
        action: Action::ClaimTask {
            task_id: Id::new_v4(),
            lease_ms: 100,
        },
    };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(
        serde_json::from_value::<Request>(json.clone()).unwrap(),
        req
    );
    for (field, value) in [
        ("schema_version", "9.0.0"),
        ("actor_id", "forged"),
        ("execute_shell", "yes"),
    ] {
        let mut bad = json.clone();
        bad[field] = value.into();
        assert!(serde_json::from_value::<Request>(bad).is_err());
    }
    let mut bad = json;
    bad["action"]["fencing_proof"] = true.into();
    assert!(serde_json::from_value::<Request>(bad).is_err());
}

// M1-P02; FR-19: all new failure codes must retain their canonical wire spelling.
#[test]
fn m1_p02_authority_and_storage_errors_are_stable() {
    for (code, wire) in [
        (ErrorCode::Unauthorized, "UNAUTHORIZED"),
        (ErrorCode::Forbidden, "FORBIDDEN"),
        (ErrorCode::NotFound, "NOT_FOUND"),
        (ErrorCode::StaleEpoch, "STALE_EPOCH"),
        (ErrorCode::IdempotencyConflict, "IDEMPOTENCY_CONFLICT"),
        (ErrorCode::Blocked, "BLOCKED"),
        (ErrorCode::StoreBusy, "STORE_BUSY"),
        (ErrorCode::StoreFailure, "STORE_FAILURE"),
        (ErrorCode::CorruptStore, "CORRUPT_STORE"),
        (ErrorCode::IntegrityFailure, "INTEGRITY_FAILURE"),
        (ErrorCode::SourceChanged, "SOURCE_CHANGED"),
        (ErrorCode::UnsafePath, "UNSAFE_PATH"),
        (ErrorCode::UnsupportedRepository, "UNSUPPORTED_REPOSITORY"),
        (ErrorCode::DestinationExists, "DESTINATION_EXISTS"),
        (ErrorCode::SupervisionFailure, "SUPERVISION_FAILURE"),
        (ErrorCode::DrainTimeout, "DRAIN_TIMEOUT"),
        (ErrorCode::DaemonUnavailable, "DAEMON_UNAVAILABLE"),
        (ErrorCode::ResponseLost, "RESPONSE_LOST"),
        (ErrorCode::ExecutionNotEnabled, "EXECUTION_NOT_ENABLED"),
    ] {
        assert_eq!(serde_json::to_value(code).unwrap(), wire);
        assert_eq!(code.as_str(), wire);
        assert!(!code.reason().is_empty() && !code.action().is_empty());
    }
}

// M2-P01; FR-05/16, INV-03: manifest schema/critical fields cannot be guessed.
#[test]
fn m2_p01_manifest_schema_is_strict() {
    let json = serde_json::json!({"schema_version":"0.1.0","policy":"local-default-v1",
        "binding":{"checkpoint_id":Id::new_v4(),"parent_id":null,"repository_id":Id::new_v4(),"task_id":Id::new_v4(),"actor_id":Id::new_v4(),"operation_id":Id::new_v4(),"epoch":1,"event_cursor":2},
        "base_commit":"1111111111111111111111111111111111111111","object_format":"sha1","files":[],"gaps":[],"reasons":["SUPERVISION_UNAVAILABLE"]});
    let manifest: SnapshotManifest = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(manifest).unwrap(), json);
    for (field, value) in [
        ("schema_version", "9.0.0"),
        ("object_format", "sha512"),
        ("execute_shell", "yes"),
    ] {
        let mut bad = json.clone();
        bad[field] = value.into();
        assert!(serde_json::from_value::<SnapshotManifest>(bad).is_err());
    }
}

// M2-P02; FR-05/09, INV-03/05: absent fields must not silently become deletions.
#[test]
fn m2_p02_file_states_require_explicit_nulls() {
    let json =
        serde_json::json!({"path":"deleted.txt","index":null,"index_oid":null,"worktree":null});
    assert!(serde_json::from_value::<SnapshotFile>(json.clone()).is_ok());
    for field in ["index", "index_oid", "worktree"] {
        let mut missing = json.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<SnapshotFile>(missing).is_err(),
            "{field}"
        );
    }
}
