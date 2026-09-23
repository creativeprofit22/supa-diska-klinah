use std::sync::Mutex;

use cleanup_core::system_change::{
    ContractError, DriverPackageName, InverseError, PriorState, Reversibility, RiskLevel,
    SystemChange, UnsupportedReason,
};

use super::{
    BoundPackages, DriverAdapter, DriverPackageStatus, DriverReader, DriverWriter,
    RawDriverPackage, classify, elevated, list_driver_packages,
};
use crate::{
    security::system_changes::HelperChange, system_change::AdapterError,
    system_change::SystemAdapter,
};

fn name(value: &str) -> DriverPackageName {
    DriverPackageName::parse(value).unwrap()
}

fn package(
    published: &str,
    original: &str,
    provider: &str,
    date: Option<&str>,
    version: Option<&str>,
) -> RawDriverPackage {
    RawDriverPackage {
        published_name: name(published),
        original_name: Some(original.into()),
        provider: Some(provider.into()),
        class: Some("Display".into()),
        driver_date: date.map(Into::into),
        driver_version: version.map(Into::into),
    }
}

#[derive(Clone)]
struct FakeReader {
    packages: Result<Vec<RawDriverPackage>, AdapterError>,
    bound: Result<BoundPackages, AdapterError>,
}

impl FakeReader {
    fn new(packages: Vec<RawDriverPackage>, bound: &[&str]) -> Self {
        Self {
            packages: Ok(packages),
            bound: Ok(BoundPackages {
                names: bound.iter().map(|name| (*name).to_owned()).collect(),
                complete: true,
            }),
        }
    }
}

impl DriverReader for FakeReader {
    fn packages(&self) -> Result<Vec<RawDriverPackage>, AdapterError> {
        self.packages.clone()
    }
    fn bound_packages(&self) -> Result<BoundPackages, AdapterError> {
        self.bound.clone()
    }
}

struct FakeWriter {
    result: Result<(), AdapterError>,
    calls: Mutex<Vec<String>>,
}

impl FakeWriter {
    fn new(result: Result<(), AdapterError>) -> Self {
        Self {
            result,
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl DriverWriter for FakeWriter {
    fn uninstall(&self, name: &DriverPackageName) -> Result<(), AdapterError> {
        self.calls.lock().unwrap().push(name.to_string());
        self.result
    }
}

/// oem1 old, oem2 newer (same orig+provider), oem3 newer still but bound,
/// oem4 same orig but other provider, oem5 tie with oem6, oem7 unparsable
/// date, oem8 different original name.
fn fixture() -> FakeReader {
    FakeReader::new(
        vec![
            package(
                "oem1.inf",
                "nv.inf",
                "NVIDIA",
                Some("01/02/2020"),
                Some("1.0.0.0"),
            ),
            package(
                "oem2.inf",
                "nv.inf",
                "NVIDIA",
                Some("05/06/2021"),
                Some("2.0.0.0"),
            ),
            package(
                "oem3.inf",
                "nv.inf",
                "nvidia",
                Some("05/06/2021"),
                Some("2.1.0.0"),
            ),
            package(
                "oem4.inf",
                "nv.inf",
                "Other",
                Some("01/01/2000"),
                Some("0.1"),
            ),
            package(
                "oem5.inf",
                "tie.inf",
                "Acme",
                Some("03/03/2022"),
                Some("1.2.3.4"),
            ),
            package(
                "oem6.inf",
                "tie.inf",
                "Acme",
                Some("03/03/2022"),
                Some("1.2.3.4"),
            ),
            package(
                "oem7.inf",
                "tie.inf",
                "Acme",
                Some("garbage"),
                Some("0.0.0.1"),
            ),
            package(
                "oem8.inf",
                "solo.inf",
                "Acme",
                Some("01/01/2001"),
                Some("1.0"),
            ),
        ],
        &["oem3.inf"],
    )
}

fn statuses(reader: &FakeReader) -> Vec<(String, DriverPackageStatus, bool)> {
    classify(
        &reader.packages.clone().unwrap(),
        &reader.bound.clone().unwrap(),
    )
    .into_iter()
    .map(|item| (item.published_name, item.status, item.deletable))
    .collect()
}

#[test]
fn classifies_superseded_in_use_and_current() {
    use DriverPackageStatus::{Current, InUse, Superseded};
    let expected = [
        ("oem1.inf", Superseded, true),
        // Superseded by oem3 (same date, higher version, provider case-insensitive).
        ("oem2.inf", Superseded, true),
        ("oem3.inf", InUse, false),
        ("oem4.inf", Current, false),
        // Exact ties never supersede each other.
        ("oem5.inf", Current, false),
        ("oem6.inf", Current, false),
        // Unparsable dates are never stale.
        ("oem7.inf", Current, false),
        ("oem8.inf", Current, false),
    ];
    let actual = statuses(&fixture());
    assert_eq!(actual.len(), expected.len());
    for ((name, status, deletable), (actual_name, actual_status, actual_deletable)) in
        expected.iter().zip(actual)
    {
        assert_eq!(*name, actual_name);
        assert_eq!(*status, actual_status, "{name}");
        assert_eq!(*deletable, actual_deletable, "{name}");
    }
}

#[test]
fn unparsable_newer_candidate_never_supersedes() {
    let reader = FakeReader::new(
        vec![
            package("oem1.inf", "a.inf", "P", Some("01/01/2020"), Some("1.0")),
            package("oem2.inf", "a.inf", "P", Some("13/45/2099"), Some("9.0")),
            package("oem3.inf", "a.inf", "P", None, Some("9.0")),
            package("oem4.inf", "a.inf", "P", Some("01/01/2020"), Some("x.y")),
        ],
        &[],
    );
    assert!(
        statuses(&reader).iter().all(|(_, status, deletable)| {
            *status == DriverPackageStatus::Current && !deletable
        })
    );
}

#[test]
fn missing_original_name_or_provider_is_never_stale() {
    let mut old = package("oem1.inf", "a.inf", "P", Some("01/01/2020"), Some("1.0"));
    old.original_name = None;
    let new = package("oem2.inf", "a.inf", "P", Some("01/01/2024"), Some("2.0"));
    let mut other = package("oem3.inf", "b.inf", "P", Some("01/01/2020"), Some("1.0"));
    other.provider = None;
    let mut other_new = package("oem4.inf", "b.inf", "P", Some("01/01/2024"), Some("2.0"));
    other_new.provider = None;
    let reader = FakeReader::new(vec![old, new, other, other_new], &[]);
    assert!(statuses(&reader).iter().all(|(_, _, deletable)| !deletable));
}

#[test]
fn incomplete_device_enumeration_disables_deletion() {
    let mut reader = fixture();
    reader.bound = Ok(BoundPackages {
        names: ["oem3.inf".to_owned()].into_iter().collect(),
        complete: false,
    });
    assert!(statuses(&reader).iter().all(|(_, _, deletable)| !deletable));
}

#[test]
fn inventory_serializes_camel_case_with_iso_dates() {
    let items = classify(&fixture().packages.unwrap(), &fixture().bound.unwrap());
    let json = serde_json::to_value(&items[0]).unwrap();
    assert_eq!(json["publishedName"], "oem1.inf");
    assert_eq!(json["originalName"], "nv.inf");
    assert_eq!(json["driverDate"], "2020-01-02");
    assert_eq!(json["driverVersion"], "1.0.0.0");
    assert_eq!(json["status"], "superseded");
    assert_eq!(json["deletable"], true);
    assert_eq!(
        serde_json::to_value(&items[6]).unwrap()["driverDate"],
        "garbage"
    );
    assert_eq!(serde_json::to_value(&items[2]).unwrap()["status"], "inUse");
}

fn delete(value: &str) -> SystemChange {
    SystemChange::DeleteDriverPackage {
        published_name: name(value),
    }
}

fn helper_delete(value: &str) -> HelperChange {
    HelperChange::DeleteDriverPackage {
        published_name: name(value),
    }
}

#[test]
fn observe_allows_only_stale_packages() {
    let adapter = DriverAdapter::new(Box::new(fixture()));
    assert_eq!(
        adapter.observe(&delete("oem1.inf")),
        Ok(PriorState::DriverPackage { present: true })
    );
    for fail_closed in ["oem3.inf", "oem5.inf", "oem7.inf", "oem8.inf"] {
        assert_eq!(
            adapter.observe(&delete(fail_closed)),
            Err(AdapterError::Unsupported(UnsupportedReason::NotPresent)),
            "{fail_closed}"
        );
    }
}

#[test]
fn absent_package_is_idempotent() {
    let adapter = DriverAdapter::new(Box::new(fixture()));
    let change = delete("oem999.inf");
    let state = adapter.observe(&change).unwrap();
    assert_eq!(state, PriorState::DriverPackage { present: false });
    assert_eq!(change.is_satisfied_by(&state), Some(true));
    assert_eq!(
        change.is_satisfied_by(&PriorState::DriverPackage { present: true }),
        Some(false)
    );

    let writer = FakeWriter::new(Ok(()));
    assert_eq!(
        elevated::apply_with(&fixture(), &writer, &helper_delete("oem999.inf")),
        Ok(())
    );
    assert!(writer.calls.lock().unwrap().is_empty());
}

#[test]
fn unknown_or_bad_names_fail_closed() {
    // Bad names are rejected by the core before reaching the adapter.
    for bad in [
        "oem1.sys",
        r"C:\Windows\INF\oem1.inf",
        "oem123456.inf",
        "OEM1.INF",
        "x.inf",
    ] {
        assert_eq!(
            DriverPackageName::parse(bad),
            Err(ContractError::InvalidDriverPackage),
            "{bad}"
        );
    }
    // A truncated package enumeration cannot prove absence.
    let many = (1..=super::MAX_PACKAGES)
        .map(|index| {
            package(
                &format!("oem{index}.inf"),
                "a.inf",
                "P",
                Some("01/01/2020"),
                Some("1.0"),
            )
        })
        .collect();
    let adapter = DriverAdapter::new(Box::new(FakeReader::new(many, &[])));
    assert_eq!(
        adapter.observe(&delete("oem99999.inf")),
        Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
    );
    // describe refuses unknown packages too.
    assert_eq!(
        DriverAdapter::new(Box::new(fixture())).describe(&delete("oem999.inf")),
        Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
    );
    // Other variants are not handled by this module.
    assert_eq!(
        elevated::observe_with(&fixture(), &HelperChange::SetHibernation { enabled: true }),
        Err(AdapterError::Failed)
    );
}

#[test]
fn describe_is_high_risk_irreversible_and_names_package() {
    let adapter = DriverAdapter::new(Box::new(fixture()));
    let impact = adapter.describe(&delete("oem1.inf")).unwrap();
    assert_eq!(impact.risk, RiskLevel::High);
    for needle in [
        "irreversible",
        "restore point",
        "NVIDIA",
        "Display",
        "1.0.0.0",
    ] {
        assert!(
            impact.effect.contains(needle),
            "{needle}: {}",
            impact.effect
        );
    }
}

#[test]
fn inverse_is_an_error_because_deletion_is_irreversible() {
    let change = delete("oem1.inf");
    assert!(matches!(
        change.reversibility(),
        Reversibility::Irreversible { .. }
    ));
    assert_eq!(
        change.inverse(&PriorState::DriverPackage { present: true }),
        Err(InverseError::Irreversible)
    );
}

#[test]
fn elevated_apply_recomputes_and_refuses_non_stale() {
    let writer = FakeWriter::new(Ok(()));
    assert_eq!(
        elevated::apply_with(&fixture(), &writer, &helper_delete("oem1.inf")),
        Ok(())
    );
    for refused in ["oem3.inf", "oem6.inf"] {
        assert_eq!(
            elevated::apply_with(&fixture(), &writer, &helper_delete(refused)),
            Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
        );
    }
    assert_eq!(*writer.calls.lock().unwrap(), vec!["oem1.inf".to_owned()]);
    assert_eq!(
        elevated::observe_with(&fixture(), &helper_delete("oem2.inf")),
        Ok(PriorState::DriverPackage { present: true })
    );
}

#[test]
fn access_denied_and_unavailable_reads_propagate() {
    let writer = FakeWriter::new(Err(AdapterError::Denied));
    assert_eq!(
        elevated::apply_with(&fixture(), &writer, &helper_delete("oem1.inf")),
        Err(AdapterError::Denied)
    );
    let mut denied = fixture();
    denied.bound = Err(AdapterError::Denied);
    assert_eq!(
        DriverAdapter::new(Box::new(denied)).observe(&delete("oem1.inf")),
        Err(AdapterError::Denied)
    );
    let mut unavailable = fixture();
    unavailable.packages = Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable));
    assert_eq!(
        DriverAdapter::new(Box::new(unavailable)).observe(&delete("oem1.inf")),
        Err(AdapterError::Unsupported(UnsupportedReason::ApiUnavailable))
    );
}

#[test]
fn live_inventory_is_read_only_and_consistent() {
    let items = list_driver_packages().unwrap();
    assert!(items.len() <= super::MAX_PACKAGES);
    for item in &items {
        assert!(DriverPackageName::parse(item.published_name.clone()).is_ok());
        assert_eq!(
            item.deletable,
            item.status == DriverPackageStatus::Superseded
        );
        for field in [
            &item.provider,
            &item.class,
            &item.original_name,
            &item.driver_version,
        ]
        .into_iter()
        .flatten()
        {
            assert!(field.chars().count() <= super::MAX_FIELD_CHARS);
        }
    }
}
