use std::io;

use cleanup_core::system_change::{
    ChangeDescription, PriorState, Privilege, Reversibility, RiskLevel, SystemChange, SystemModule,
    UnsupportedReason,
};
use windows::Win32::{
    Foundation::{E_ACCESSDENIED, E_FAIL, REGDB_E_CLASSNOTREG},
    System::Wmi::{WBEM_E_ACCESS_DENIED, WBEM_E_INVALID_CLASS, WBEM_E_INVALID_NAMESPACE},
};

use super::*;

struct FakeSource(Result<Vec<RawRestorePoint>, WmiFailure>);

impl RestorePointSource for FakeSource {
    fn query(&self) -> Result<Vec<RawRestorePoint>, WmiFailure> {
        self.0.clone()
    }
}

struct FakeReader(Result<ProtectionValues, io::ErrorKind>);

impl ProtectionReader for FakeReader {
    fn read(&self) -> io::Result<ProtectionValues> {
        self.0.map_err(io::Error::from)
    }
}

fn change() -> SystemChange {
    SystemChange::CreateRestorePoint {
        description: ChangeDescription::parse("Before tuning").unwrap(),
    }
}

fn adapter(values: ProtectionValues) -> RestoreAdapter<FakeReader> {
    RestoreAdapter::with_reader(FakeReader(Ok(values)))
}

fn raw(sequence: u32, time: &str, kind: u32) -> RawRestorePoint {
    RawRestorePoint {
        sequence_number: Some(sequence),
        description: Some(format!("Point {sequence}")),
        creation_time: Some(time.to_owned()),
        restore_point_type: Some(kind),
    }
}

#[test]
fn query_is_the_documented_constant() {
    assert_eq!(
        wmi::QUERY,
        "SELECT SequenceNumber, Description, CreationTime, RestorePointType FROM SystemRestore"
    );
}

#[test]
fn parses_valid_cim_datetimes() {
    assert_eq!(parse_cim_datetime("19700101000000.000000+000"), Some(0));
    assert_eq!(
        parse_cim_datetime("20240229123045.123456+000"),
        Some(1_709_209_845)
    );
    // Local 14:30:45 at UTC+120 minutes is 12:30:45 UTC.
    assert_eq!(
        parse_cim_datetime("20240229143045.000000+120"),
        Some(1_709_209_845)
    );
    // Local 07:30:45 at UTC-300 minutes is 12:30:45 UTC.
    assert_eq!(
        parse_cim_datetime("20240229073045.000000-300"),
        Some(1_709_209_845)
    );
    assert_eq!(
        parse_cim_datetime("20000101000000.000000+000"),
        Some(946_684_800)
    );
}

#[test]
fn rejects_malformed_cim_datetimes() {
    for value in [
        "",
        "20240229123045",
        "20240229123045.123456+00",
        "20240229123045.123456+0000",
        "20240229123045,123456+000",
        "20240229123045.123456*000",
        "2024022912304a.123456+000",
        "20240230123045.000000+000",
        "20230229123045.000000+000",
        "20241301123045.000000+000",
        "20240001123045.000000+000",
        "20240100123045.000000+000",
        "20240101243045.000000+000",
        "20240101126045.000000+000",
        "20240101123060.000000+000",
        "20240101123045.000000+999",
        "********123045.000000+000",
        "２０240229123045.00000+000",
    ] {
        assert_eq!(parse_cim_datetime(value), None, "{value:?}");
    }
}

#[test]
fn maps_restore_point_type_labels() {
    for (value, kind) in [
        (0, RestorePointKind::ApplicationInstall),
        (1, RestorePointKind::ApplicationUninstall),
        (10, RestorePointKind::DeviceDriverInstall),
        (12, RestorePointKind::ModifySettings),
        (13, RestorePointKind::CancelledOperation),
        (7, RestorePointKind::Other),
        (u32::MAX, RestorePointKind::Other),
    ] {
        assert_eq!(RestorePointKind::from_type(value), kind);
    }
    assert_eq!(
        serde_json::to_value(RestorePointKind::DeviceDriverInstall).unwrap(),
        "deviceDriverInstall"
    );
}

#[test]
fn wmi_access_denied_reports_requires_administrator() {
    for code in [WBEM_E_ACCESS_DENIED.0, E_ACCESSDENIED.0] {
        let list = list_restore_points_with(&FakeSource(Err(WmiFailure(code)))).unwrap();
        assert_eq!(list.status, RestorePointListStatus::RequiresAdministrator);
        assert!(list.points.is_empty());
    }
}

#[test]
fn missing_namespace_or_class_reports_unavailable() {
    for code in [
        WBEM_E_INVALID_NAMESPACE.0,
        WBEM_E_INVALID_CLASS.0,
        REGDB_E_CLASSNOTREG.0,
    ] {
        let list = list_restore_points_with(&FakeSource(Err(WmiFailure(code)))).unwrap();
        assert_eq!(list.status, RestorePointListStatus::Unavailable);
        assert!(list.points.is_empty());
    }
}

#[test]
fn other_wmi_failures_are_errors() {
    assert_eq!(
        list_restore_points_with(&FakeSource(Err(WmiFailure(E_FAIL.0)))),
        Err(RestoreError::Wmi(E_FAIL.0))
    );
}

#[test]
fn successful_listing_is_sorted_bounded_and_labelled() {
    let mut points = vec![
        raw(3, "20240229123045.000000+000", 12),
        raw(7, "not a date", 0),
        RawRestorePoint {
            sequence_number: None,
            ..raw(99, "20240229123045.000000+000", 1)
        },
        RawRestorePoint {
            description: Some("x".repeat(1000)),
            restore_point_type: None,
            ..raw(5, "20240229123045.000000+000", 0)
        },
    ];
    points.extend((1000..1400).map(|sequence| raw(sequence, "", 13)));
    let list = list_restore_points_with(&FakeSource(Ok(points))).unwrap();

    assert_eq!(list.status, RestorePointListStatus::Available);
    // The first 256 raw rows are kept; the row without a sequence number is dropped.
    assert_eq!(list.points.len(), MAX_RESTORE_POINTS - 1);
    assert!(
        list.points
            .windows(2)
            .all(|pair| pair[0].sequence_number > pair[1].sequence_number)
    );
    let three = list.points.iter().find(|p| p.sequence_number == 3).unwrap();
    assert_eq!(three.created_at, Some(1_709_209_845));
    assert_eq!(three.kind, RestorePointKind::ModifySettings);
    assert_eq!(three.description, "Point 3");
    let seven = list.points.iter().find(|p| p.sequence_number == 7).unwrap();
    assert_eq!(seven.created_at, None);
    let five = list.points.iter().find(|p| p.sequence_number == 5).unwrap();
    assert_eq!(five.description.chars().count(), MAX_DESCRIPTION_CHARS);
    assert_eq!(five.kind, RestorePointKind::Other);

    let json = serde_json::to_value(&list).unwrap();
    assert_eq!(json["status"], "available");
    assert!(json["points"][0].get("sequenceNumber").is_some());
    assert!(json["points"][0].get("createdAt").is_some());
}

#[test]
fn protection_defaults_and_flags() {
    let default = RestoreProtection::from_values(ProtectionValues::default());
    assert_eq!(
        default,
        RestoreProtection {
            policy_disabled: false,
            protection_enabled: None,
            creation_frequency_minutes: DEFAULT_CREATION_FREQUENCY_MINUTES,
        }
    );
    let configured = RestoreProtection::from_values(ProtectionValues {
        disable_sr: Some(0),
        disable_config: Some(1),
        rp_session_interval: Some(1),
        creation_frequency_minutes: Some(0),
    });
    assert!(configured.policy_disabled);
    assert_eq!(configured.protection_enabled, Some(true));
    assert_eq!(configured.creation_frequency_minutes, 0);
    let off = RestoreProtection::from_values(ProtectionValues {
        rp_session_interval: Some(0),
        ..ProtectionValues::default()
    });
    assert_eq!(off.protection_enabled, Some(false));
    let json = serde_json::to_value(off).unwrap();
    assert_eq!(json["policyDisabled"], false);
    assert_eq!(json["protectionEnabled"], false);
    assert_eq!(json["creationFrequencyMinutes"], 1440);
}

#[test]
fn observe_matrix_policy_protection_available() {
    // Registry-driven only: no build or edition dependency.
    let policy = adapter(ProtectionValues {
        disable_sr: Some(1),
        rp_session_interval: Some(1),
        ..ProtectionValues::default()
    });
    assert_eq!(
        policy.observe(&change()),
        Err(AdapterError::Unsupported(UnsupportedReason::ManagedDevice))
    );
    // Policy wins even when protection is also off.
    let both = adapter(ProtectionValues {
        disable_sr: Some(1),
        rp_session_interval: Some(0),
        ..ProtectionValues::default()
    });
    assert_eq!(
        both.observe(&change()),
        Err(AdapterError::Unsupported(UnsupportedReason::ManagedDevice))
    );
    let off = adapter(ProtectionValues {
        rp_session_interval: Some(0),
        ..ProtectionValues::default()
    });
    assert_eq!(
        off.observe(&change()),
        Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable))
    );
    for interval in [Some(1), None] {
        let available = adapter(ProtectionValues {
            rp_session_interval: interval,
            ..ProtectionValues::default()
        });
        assert_eq!(available.observe(&change()), Ok(PriorState::NotApplicable));
    }
}

#[test]
fn observe_maps_registry_failures() {
    let denied = RestoreAdapter::with_reader(FakeReader(Err(io::ErrorKind::PermissionDenied)));
    assert_eq!(denied.observe(&change()), Err(AdapterError::Denied));
    let failed = RestoreAdapter::with_reader(FakeReader(Err(io::ErrorKind::Other)));
    assert_eq!(failed.observe(&change()), Err(AdapterError::Failed));
}

#[test]
fn other_variants_fail_closed() {
    let other = SystemChange::RemoveScanSchedule {
        schedule_id: cleanup_core::system_change::ScheduleId::parse(
            "0f8fad5b-d9cb-469f-a165-70867728950e",
        )
        .unwrap(),
    };
    let adapter = adapter(ProtectionValues::default());
    assert_eq!(adapter.observe(&other), Err(AdapterError::Failed));
    assert_eq!(adapter.describe(&other), Err(AdapterError::Failed));
}

#[test]
fn describe_is_truthful_and_low_risk() {
    let impact = adapter(ProtectionValues {
        creation_frequency_minutes: Some(60),
        ..ProtectionValues::default()
    })
    .describe(&change())
    .unwrap();
    assert_eq!(impact.risk, RiskLevel::Low);
    assert!(impact.effect.contains("Before tuning"));
    assert!(impact.effect.contains("administrator"));
    assert!(impact.effect.contains("last 60 minutes"));
    let fallback = RestoreAdapter::with_reader(FakeReader(Err(io::ErrorKind::Other)))
        .describe(&change())
        .unwrap();
    assert!(fallback.effect.contains("recently"));
}

#[test]
fn never_satisfied_and_irreversible_helper_change() {
    let adapter = adapter(ProtectionValues::default());
    for state in [
        PriorState::NotApplicable,
        PriorState::Enabled { enabled: true },
    ] {
        assert!(!adapter.is_satisfied(&change(), &state));
    }
    let change = change();
    assert_eq!(change.module(), SystemModule::Restore);
    assert_eq!(change.privilege(), Privilege::Helper);
    assert!(matches!(
        change.reversibility(),
        Reversibility::Irreversible { .. }
    ));
    assert_eq!(
        change.inverse(&PriorState::NotApplicable).ok().flatten(),
        None
    );
}

#[test]
fn live_protection_status_reads_without_elevation() {
    let protection = get_restore_protection().unwrap();
    // Observing must agree with the live status and never mutate anything.
    let observed = RestoreAdapter::new().observe(&change());
    if protection.policy_disabled {
        assert_eq!(
            observed,
            Err(AdapterError::Unsupported(UnsupportedReason::ManagedDevice))
        );
    } else if protection.protection_enabled == Some(false) {
        assert_eq!(
            observed,
            Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable))
        );
    } else {
        assert_eq!(observed, Ok(PriorState::NotApplicable));
    }
}

#[test]
fn live_listing_reports_a_known_status() {
    let list = list_restore_points().unwrap();
    assert!(list.points.len() <= MAX_RESTORE_POINTS);
    if list.status != RestorePointListStatus::Available {
        assert!(list.points.is_empty());
    }
}
