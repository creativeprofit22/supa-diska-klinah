use super::*;
use crate::protection::net::fake::CountingTransport;
use crate::protection::test_support::temp_dir;

fn service(label: &str) -> (PathBuf, ProtectionService<CountingTransport>) {
    let app_data = fs::canonicalize(temp_dir(label)).unwrap();
    let service = ProtectionService::with_transport(
        &app_data,
        CountingTransport::default(),
        KnownLocations::default(),
    )
    .unwrap();
    (app_data, service)
}

#[test]
fn import_from_folder_without_pack_files_reports_missing_pack() {
    let (dir, service) = service("svc-import-empty");
    let folder = dir.join("empty");
    fs::create_dir(&folder).unwrap();
    let error = service.import_rules(&folder).unwrap_err();
    assert_eq!(error, ProtectionError::RulePackMissing);
    assert_eq!(
        error.to_string(),
        "the chosen folder must contain pack.json and pack.sig"
    );
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        serde_json::json!("rulePackMissing")
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn import_of_equal_sequence_pack_is_rejected_as_not_newer() {
    use crate::protection::test_support::{signed_pack, test_verifier};
    let (dir, service) = service("svc-import-equal");
    // The compiled key may be a release key; swap in a store trusting the test key.
    let (baseline_pack, baseline_sig) = signed_pack(5, "test baseline");
    *service.rules.lock().unwrap() = RulesStore::open_with(
        dir.join("test-rules"),
        test_verifier(),
        baseline_pack,
        baseline_sig,
    )
    .unwrap();
    let active = service.overview().unwrap().rules.sequence;
    let folder = dir.join("pack");
    fs::create_dir(&folder).unwrap();
    let (pack, sig) = signed_pack(active, "same sequence");
    fs::write(folder.join("pack.json"), pack).unwrap();
    fs::write(folder.join("pack.sig"), sig).unwrap();

    let error = service.import_rules(&folder).unwrap_err();

    assert!(
        matches!(error, ProtectionError::RulesRejected(ref r) if r.reason == RuleRejection::NotNewer),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("not newer"), "{message}");
    assert!(
        message.ends_with("the current pack is still active"),
        "{message}"
    );
    assert!(
        !message.contains(&folder.display().to_string()),
        "{message}"
    );
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        serde_json::json!({ "rulesRejected": "notNewer" })
    );
    assert_eq!(service.overview().unwrap().rules.sequence, active);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn defaults_are_offline_and_network_features_make_zero_calls() {
    let (dir, service) = service("svc-offline");
    let overview = service.overview().unwrap();
    assert_eq!(
        overview.settings.network,
        ProtectionNetworkPolicy::default()
    );
    assert!(!overview.settings.amsi_enabled);
    assert_eq!(
        overview.rules.sequence, 1,
        "embedded baseline active offline"
    );
    assert_eq!(
        service.download_rules().unwrap_err(),
        ProtectionError::NetworkDisabled
    );
    assert_eq!(
        service
            .check_password(Zeroizing::new(b"password".to_vec()))
            .unwrap_err(),
        ProtectionError::NetworkDisabled
    );
    assert_eq!(service.net().transport().count(), 0);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn network_failure_keeps_rules_and_scanning_works_offline() {
    let (dir, service) = service("svc-netfail");
    service
        .set_network_policy(
            ProtectionNetworkPolicy {
                rule_download: true,
                password_breach_check: true,
            },
            false,
        )
        .unwrap();
    // The counting fake has no scripted responses, so every call fails as unreachable.
    assert_eq!(
        service.download_rules().unwrap_err(),
        ProtectionError::NetworkFailed
    );
    assert_eq!(
        service
            .check_password(Zeroizing::new(b"password".to_vec()))
            .unwrap_err(),
        ProtectionError::NetworkFailed
    );
    assert_eq!(service.overview().unwrap().rules.sequence, 1);
    let folder = dir.join("scan");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("invoice.pdf.exe"), b"x").unwrap();
    let report = service.scan(ScanScope::Folder, Some(folder)).unwrap();
    assert_eq!(report.summary.heuristic, 1);
    let _ = fs::remove_dir_all(dir);
}

/// Blocks inside the first request until the test releases it.
struct GatedTransport {
    entered: Mutex<std::sync::mpsc::Sender<()>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl Transport for GatedTransport {
    fn get(
        &self,
        _: &'static str,
        _: &str,
        _: &'static str,
        _: usize,
    ) -> Result<Vec<u8>, NetError> {
        let _ = self.entered.lock().unwrap().send(());
        let _ = self
            .release
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(30));
        Err(NetError::Unreachable)
    }
}

#[test]
fn slow_download_does_not_block_overview_or_rules() {
    use std::sync::mpsc::channel;
    use std::time::Duration;
    let app_data = fs::canonicalize(temp_dir("svc-slow-dl")).unwrap();
    let (entered_tx, entered_rx) = channel();
    let (release_tx, release_rx) = channel();
    let transport = GatedTransport {
        entered: Mutex::new(entered_tx),
        release: Mutex::new(release_rx),
    };
    let service =
        ProtectionService::with_transport(&app_data, transport, KnownLocations::default()).unwrap();
    service
        .set_network_policy(
            ProtectionNetworkPolicy {
                rule_download: true,
                password_breach_check: false,
            },
            false,
        )
        .unwrap();

    let service = &service;
    let (overview, restore, download) = std::thread::scope(|scope| {
        let download = scope.spawn(|| service.download_rules());
        entered_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("download never reached the network");
        // The download is now paused mid-request; other callers must still get through.
        let (done_tx, done_rx) = channel();
        scope.spawn(move || {
            let _ = done_tx.send((service.overview(), service.restore_previous_rules()));
        });
        let observed = done_rx.recv_timeout(Duration::from_secs(10));
        release_tx.send(()).unwrap();
        let (overview, restore) = observed.expect("overview blocked behind the download");
        (overview, restore, download.join().unwrap())
    });
    assert_eq!(overview.unwrap().rules.sequence, 1);
    assert!(
        matches!(restore.unwrap_err(), ProtectionError::RulesRejected(ref r) if r.reason == RuleRejection::NoPrevious),
        "no previous pack to restore"
    );
    assert_eq!(download.unwrap_err(), ProtectionError::NetworkFailed);
    assert_eq!(service.overview().unwrap().rules.sequence, 1);
    let _ = fs::remove_dir_all(app_data);
}

#[test]
fn settings_persist_and_fail_closed_when_corrupt() {
    let (dir, service) = service("svc-settings");
    service
        .set_network_policy(
            ProtectionNetworkPolicy {
                rule_download: true,
                password_breach_check: false,
            },
            true,
        )
        .unwrap();
    drop(service);
    let reopened = ProtectionService::with_transport(
        &dir,
        CountingTransport::default(),
        KnownLocations::default(),
    )
    .unwrap();
    let settings = reopened.overview().unwrap().settings;
    assert!(
        settings.network.rule_download
            && settings.amsi_enabled
            && !settings.network.password_breach_check
    );
    drop(reopened);
    fs::write(dir.join("protection/settings.json"), br#"{"network":{"ruleDownload":true,"passwordBreachCheck":true},"amsiEnabled":true,"telemetry":true}"#).unwrap();
    let reopened = ProtectionService::with_transport(
        &dir,
        CountingTransport::default(),
        KnownLocations::default(),
    )
    .unwrap();
    assert_eq!(
        reopened.overview().unwrap().settings,
        ProtectionSettings::default()
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn findings_are_addressed_by_opaque_ids_through_quarantine_and_restore() {
    let (dir, service) = service("svc-flow");
    let folder = dir.join("scan");
    fs::create_dir(&folder).unwrap();
    let file = folder.join("invoice.pdf.exe");
    fs::write(&file, b"pretend program").unwrap();
    let report = service
        .scan(ScanScope::Folder, Some(folder.clone()))
        .unwrap();
    let finding = report.findings.iter().find(|f| f.can_quarantine).unwrap();
    assert_eq!(finding.id.len(), 32);
    assert_eq!(
        service.quarantine_finding("../../x").unwrap_err(),
        ProtectionError::InvalidInput
    );
    assert_eq!(
        service.quarantine_finding(&"0".repeat(32)).unwrap_err(),
        ProtectionError::NotFound
    );
    assert!(
        service
            .quarantine_prompt(&finding.id, crate::i18n::strings(crate::i18n::Locale::En))
            .unwrap()
            .contains("invoice.pdf.exe")
    );
    let spanish = service
        .quarantine_prompt(
            &finding.id,
            crate::i18n::strings(crate::i18n::Locale::Es419),
        )
        .unwrap();
    assert!(
        spanish.starts_with("¿Mover este archivo a cuarentena?")
            && spanish.contains("invoice.pdf.exe")
    );
    let entry = service.quarantine_finding(&finding.id).unwrap();
    assert!(!file.exists());
    assert!(
        service
            .last_scan()
            .unwrap()
            .unwrap()
            .findings
            .iter()
            .all(|f| f.id != finding.id)
    );
    assert!(
        service
            .restore_prompt(&entry.id, crate::i18n::strings(crate::i18n::Locale::En))
            .unwrap()
            .contains("nothing is overwritten")
    );
    assert!(
        service
            .restore_prompt(&entry.id, crate::i18n::strings(crate::i18n::Locale::Es419))
            .unwrap()
            .contains("no se sobrescribe nada")
    );
    assert_eq!(service.restore(&entry.id).unwrap(), file.to_string_lossy());
    assert_eq!(fs::read(&file).unwrap(), b"pretend program");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn external_responses_never_offer_or_perform_quarantine() {
    let (dir, service) = service("svc-external");
    let folder = dir.join("scan");
    fs::create_dir(&folder).unwrap();
    let file = folder.join("invoice.pdf.exe");
    fs::write(&file, b"pretend program").unwrap();
    service
        .scan(ScanScope::Folder, Some(folder.clone()))
        .unwrap();
    let original = service
        .last_scan
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .1
        .values()
        .next()
        .unwrap()
        .clone();
    let external = ScannedFinding {
        evidence: Evidence::External {
            provider: amsi::PROVIDER.into(),
            observed_at: utc_now(),
            detail: "The installed antivirus did not report this content. This is not a guarantee."
                .into(),
        },
        ..original
    };
    assert!(external.sha256.is_some());
    assert_eq!(
        quarantine_label(&external.evidence),
        None,
        "external rows must not offer quarantine"
    );
    let id = "e".repeat(32);
    service
        .last_scan
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .1
        .insert(id.clone(), external);

    assert_eq!(
        service.quarantine_finding(&id).unwrap_err(),
        ProtectionError::InvalidInput
    );
    assert!(file.exists());
    assert!(service.quarantine_list().is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn allowlist_applies_to_later_scans_and_rejects_non_heuristics() {
    let (dir, service) = service("svc-allow");
    let folder = dir.join("scan");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("image.png"), b"MZ\x90\x00").unwrap();
    let report = service
        .scan(ScanScope::Folder, Some(folder.clone()))
        .unwrap();
    let id = report.findings[0].id.clone();
    assert_eq!(service.allow_hash(&id).unwrap().allowlist.len(), 1);
    let again = service
        .scan(ScanScope::Folder, Some(folder.clone()))
        .unwrap();
    assert_eq!((again.summary.heuristic, again.summary.allowlisted), (0, 1));
    assert!(service.clear_allowlist().unwrap().allowlist.is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn scan_status_reports_a_running_scan_and_its_progress() {
    let (dir, service) = service("svc-status");
    let folder = dir.join("scan");
    fs::create_dir(&folder).unwrap();
    for i in 0..3000 {
        fs::write(folder.join(format!("file{i}.txt")), vec![b'x'; 4096]).unwrap();
    }
    assert_eq!(service.scan_status().unwrap(), ScanStatus::default());
    let observed = std::thread::scope(|scope| {
        let handle = scope.spawn(|| service.scan(ScanScope::Folder, Some(folder.clone())));
        let mut observed = None;
        while !handle.is_finished() {
            let status = service.scan_status().unwrap();
            if status.running && status.files_scanned > 0 {
                observed = Some(status);
                break;
            }
            std::thread::yield_now();
        }
        let report = handle.join().unwrap().unwrap();
        assert_eq!(report.summary.files_scanned, 3000);
        observed
    });
    let observed = observed.expect("scan finished before a running status was observed");
    assert!(observed.files_scanned > 0 && observed.files_scanned <= 3000);
    assert!(!service.scan_status().unwrap().running);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn scope_and_folder_must_agree() {
    let (dir, service) = service("svc-scope");
    assert_eq!(
        service.scan(ScanScope::Folder, None).unwrap_err(),
        ProtectionError::InvalidInput
    );
    assert_eq!(
        service
            .scan(ScanScope::Quick, Some(dir.clone()))
            .unwrap_err(),
        ProtectionError::InvalidInput
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn utc_now_is_well_formed() {
    let now = utc_now();
    assert_eq!(now.len(), 20);
    assert!(now.starts_with("20") && now.ends_with('Z'));
}
