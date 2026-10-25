use baton_protocol::{Diagnostic, ErrorCode, RecoveryQuality, SchemaVersion};

// M0-001; FR-19, NFR-08. Removing strict decoding must fail this test.
#[test]
fn m0_001_rejects_incompatible_or_extended_diagnostics() {
    for json in [
        r#"{"schema_version":"0.2.0","code":"INVALID_INPUT"}"#,
        r#"{"schema_version":"0.1.0","code":"INVALID_INPUT","authority":true}"#,
        r#"{"schema_version":"0.1.0","code":"UNRECOGNIZED"}"#,
        r#"{"code":"INVALID_INPUT"}"#,
    ] {
        assert!(serde_json::from_str::<Diagnostic>(json).is_err(), "{json}");
    }
}

// M0-002; FR-19. Changing canonical wire codes or losing guidance must fail.
#[test]
fn m0_002_diagnostics_have_stable_codes_and_safe_guidance() {
    for (code, wire) in [
        (ErrorCode::InvalidInput, "INVALID_INPUT"),
        (ErrorCode::UnsupportedSchema, "UNSUPPORTED_SCHEMA"),
        (ErrorCode::Io, "IO_ERROR"),
        (ErrorCode::GitUnavailable, "GIT_UNAVAILABLE"),
        (ErrorCode::GitFailed, "GIT_FAILED"),
        (ErrorCode::AdapterProtocol, "ADAPTER_PROTOCOL"),
        (ErrorCode::AdapterTimeout, "ADAPTER_TIMEOUT"),
        (ErrorCode::AdapterRejected, "ADAPTER_REJECTED"),
    ] {
        let diagnostic = Diagnostic::new(code);
        let json = serde_json::to_value(&diagnostic).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"schema_version":"0.1.0", "code":wire})
        );
        assert_eq!(
            serde_json::from_value::<Diagnostic>(json).unwrap(),
            diagnostic
        );
        assert_eq!(diagnostic.schema_version, SchemaVersion::V0_1_0);
        assert!(!code.reason().is_empty());
        assert!(!code.action().is_empty());
        assert!(diagnostic.to_string().contains(wire));
    }
}

// M0-003; FR-13, INV-08, INV-09. Collapsing outcomes must fail this test.
#[test]
fn m0_003_quality_labels_are_explicit_and_strict() {
    for (quality, wire) in [
        (RecoveryQuality::Verified, "VERIFIED"),
        (RecoveryQuality::Degraded, "DEGRADED"),
        (RecoveryQuality::Blocked, "BLOCKED"),
    ] {
        assert_eq!(serde_json::to_value(quality).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<RecoveryQuality>(wire.into()).unwrap(),
            quality
        );
    }
    assert!(serde_json::from_str::<RecoveryQuality>(r#""SUCCESS""#).is_err());
}
