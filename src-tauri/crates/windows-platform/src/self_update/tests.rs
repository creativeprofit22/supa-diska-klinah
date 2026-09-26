use std::sync::{Arc, Mutex};

use ed25519_dalek::{Signer, SigningKey};
use protection_core::{UpdateCheckPolicy, UpdateSigning, UpdateVerifier, to_hex};
use sha2::{Digest, Sha256};

use super::*;
use crate::protection::net::NetError;
use crate::protection::net::fake::CountingTransport;

const NOW: u64 = 1_790_000_000;
const DAY: u64 = 86_400;
const THUMB_A: &str = "0123456789ABCDEF0123456789ABCDEF01234567";
const THUMB_B: &str = "89ABCDEF0123456789ABCDEF0123456789ABCDEF";
const ON: UpdateCheckPolicy = UpdateCheckPolicy { enabled: true };
const OFF: UpdateCheckPolicy = UpdateCheckPolicy { enabled: false };

fn clock() -> u64 {
    NOW
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

fn verifier() -> UpdateVerifier {
    UpdateVerifier::from_key_file(&to_hex(key().verifying_key().as_bytes())).unwrap()
}

fn installer_bytes() -> Vec<u8> {
    b"MZ fake NSIS installer payload".repeat(100)
}

struct Release {
    version: &'static str,
    installer: Vec<u8>,
    /// Size/hash advertised by the manifest (defaults to the real installer).
    advertised: Option<(u64, String)>,
    signing: &'static str,
    thumbprint: Option<&'static str>,
}

impl Default for Release {
    fn default() -> Self {
        Self {
            version: "0.2.0",
            installer: installer_bytes(),
            advertised: None,
            signing: "none",
            thumbprint: None,
        }
    }
}

impl Release {
    fn manifest(&self) -> Vec<u8> {
        let (size, sha) = self.advertised.clone().unwrap_or_else(|| {
            (
                self.installer.len() as u64,
                to_hex(&Sha256::digest(&self.installer)),
            )
        });
        let thumb = self
            .thumbprint
            .map(|t| format!(r#","signerThumbprint":"{t}""#))
            .unwrap_or_default();
        format!(
            r#"{{"format":1,"version":"{v}","minimumVersion":"0.1.0","installer":{{"name":"Supa-Diska-Klinah_{v}_x64-setup.exe","size":{size},"sha256":"{sha}"}},"notBefore":{nb},"expires":{ex},"signing":"{s}"{thumb}}}"#,
            v = self.version,
            nb = NOW - DAY,
            ex = NOW + 30 * DAY,
            s = self.signing,
        )
        .into_bytes()
    }

    fn signature(&self, manifest: &[u8]) -> Vec<u8> {
        to_hex(&key().sign(manifest).to_bytes()).into_bytes()
    }

    /// Responses for check (manifest, signature) then download (installer).
    fn responses(&self) -> Vec<Result<Vec<u8>, NetError>> {
        let manifest = self.manifest();
        let signature = self.signature(&manifest);
        vec![Ok(manifest), Ok(signature), Ok(self.installer.clone())]
    }
}

#[derive(Default)]
struct ProbeState {
    running: Mutex<Option<AuthenticodeVerdict>>,
    candidate: Mutex<Option<AuthenticodeVerdict>>,
}

struct FakeProbe(Arc<ProbeState>, PathBuf);

impl AuthenticodeProbe for FakeProbe {
    fn verdict(&self, path: &Path, _file: &File) -> AuthenticodeVerdict {
        let slot = if path == self.1 {
            &self.0.running
        } else {
            &self.0.candidate
        };
        slot.lock()
            .unwrap()
            .clone()
            .unwrap_or(AuthenticodeVerdict::Unsigned)
    }
}

#[derive(Default)]
struct LaunchState {
    launched: Mutex<Vec<PathBuf>>,
    fail: Mutex<bool>,
}

struct FakeLauncher(Arc<LaunchState>);

impl InstallerLauncher for FakeLauncher {
    fn launch(&self, installer: &Path) -> Result<(), ()> {
        // While launching, the staged file must still be locked against writes.
        assert!(
            OpenOptions::new().write(true).open(installer).is_err(),
            "installer must be write-locked during launch"
        );
        self.0
            .launched
            .lock()
            .unwrap()
            .push(installer.to_path_buf());
        if *self.0.fail.lock().unwrap() {
            Err(())
        } else {
            Ok(())
        }
    }
}

struct Harness {
    dir: PathBuf,
    probe: Arc<ProbeState>,
    launch: Arc<LaunchState>,
}

impl Harness {
    fn new(name: &str) -> Self {
        let mut nonce = [0_u8; 8];
        getrandom::fill(&mut nonce).unwrap();
        let dir = std::env::temp_dir().join(format!("sdk-self-update-{name}-{}", to_hex(&nonce)));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.exe"), b"running app").unwrap();
        Self {
            dir,
            probe: Arc::default(),
            launch: Arc::default(),
        }
    }

    /// A service after startup recovery, as the app runs it.
    fn service(
        &self,
        running: &str,
        responses: Vec<Result<Vec<u8>, NetError>>,
    ) -> UpdateService<CountingTransport> {
        self.service_with(running, CountingTransport::with(responses))
    }

    fn service_with(
        &self,
        running: &str,
        transport: CountingTransport,
    ) -> UpdateService<CountingTransport> {
        let service = self.starting_with(running, transport);
        service.recover_on_startup();
        service
    }

    /// A freshly started service whose startup recovery has not run yet.
    fn starting(&self, running: &str) -> UpdateService<CountingTransport> {
        self.starting_with(running, CountingTransport::default())
    }

    fn starting_with(
        &self,
        running: &str,
        transport: CountingTransport,
    ) -> UpdateService<CountingTransport> {
        let exe = self.dir.join("app.exe");
        UpdateService::new(UpdateServiceConfig {
            app_data: self.dir.clone(),
            running_version: running.into(),
            running_exe: exe.clone(),
            transport,
            verifier: Ok(verifier()),
            clock,
            probe: Box::new(FakeProbe(Arc::clone(&self.probe), exe)),
            launcher: Box::new(FakeLauncher(Arc::clone(&self.launch))),
        })
        .unwrap()
    }

    fn staged_files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.dir.join("updates"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n != "state.json")
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn set_candidate(&self, verdict: AuthenticodeVerdict) {
        *self.probe.candidate.lock().unwrap() = Some(verdict);
    }

    fn set_running(&self, verdict: AuthenticodeVerdict) {
        *self.probe.running.lock().unwrap() = Some(verdict);
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn valid(thumbprint: &str) -> AuthenticodeVerdict {
    AuthenticodeVerdict::Valid {
        thumbprint: thumbprint.into(),
    }
}

const INSTALLER: &str = "Supa-Diska-Klinah_0.2.0_x64-setup.exe";

#[test]
fn check_needs_the_opt_in_and_a_configured_key() {
    let h = Harness::new("optin");
    let service = h.service("0.1.0", Release::default().responses());
    assert_eq!(service.check(OFF), Err(UpdateError::NotPermitted));
    assert_eq!(
        service.net.transport().count(),
        0,
        "nothing is contacted while off"
    );

    let exe = h.dir.join("app.exe");
    let unconfigured = UpdateService::new(UpdateServiceConfig {
        app_data: h.dir.clone(),
        running_version: "0.1.0".into(),
        running_exe: exe.clone(),
        transport: CountingTransport::default(),
        verifier: UpdateVerifier::from_key_file("unconfigured\n"),
        clock,
        probe: Box::new(FakeProbe(Arc::clone(&h.probe), exe)),
        launcher: Box::new(FakeLauncher(Arc::clone(&h.launch))),
    })
    .unwrap();
    unconfigured.recover_on_startup();
    assert!(!unconfigured.status().configured);
    assert_eq!(unconfigured.check(ON), Err(UpdateError::NotConfigured));
}

#[test]
fn check_reports_available_up_to_date_and_bad_manifests() {
    let h = Harness::new("check");
    let service = h.service("0.1.0", Release::default().responses());
    let status = service.check(ON).unwrap();
    assert_eq!(
        status.update,
        UpdateState::Available {
            version: "0.2.0".into(),
            size: installer_bytes().len() as u64
        }
    );

    // Same version (and so any rollback) is "up to date", never offered.
    let service = h.service("0.2.0", Release::default().responses());
    assert_eq!(service.check(ON).unwrap().update, UpdateState::UpToDate);

    // Tampered manifest: signature no longer matches.
    let release = Release::default();
    let manifest = release.manifest();
    let signature = release.signature(&manifest);
    let mut tampered = manifest.clone();
    let index = tampered.iter().position(|&b| b == b'1').unwrap();
    tampered[index] = b'9';
    let service = h.service("0.1.0", vec![Ok(tampered), Ok(signature)]);
    assert_eq!(service.check(ON), Err(UpdateError::BadManifest));
    assert_eq!(service.status().update, UpdateState::Idle);

    let service = h.service("0.1.0", vec![Err(NetError::Unreachable)]);
    assert_eq!(service.check(ON), Err(UpdateError::Network));
}

#[test]
fn download_verifies_and_stages_the_installer() {
    let h = Harness::new("download");
    let service = h.service("0.1.0", Release::default().responses());
    assert_eq!(service.download(ON), Err(UpdateError::NoUpdateChecked));
    service.check(ON).unwrap();
    assert_eq!(service.download(OFF), Err(UpdateError::NotPermitted));
    let status = service.download(ON).unwrap();
    assert_eq!(
        status.update,
        UpdateState::Verified {
            version: "0.2.0".into()
        }
    );
    assert_eq!(h.staged_files(), vec![INSTALLER]);
    let requests = service.net.transport().requests.lock().unwrap().clone();
    assert_eq!(
        requests.last().unwrap().1,
        "/creativeprofit22/supa-diska-klinah/releases/download/v0.2.0/Supa-Diska-Klinah_0.2.0_x64-setup.exe"
    );
}

#[test]
fn tampered_or_wrong_sized_installers_are_deleted() {
    let h = Harness::new("tamper");
    let mut release = Release::default();
    let real = release.installer.clone();
    release.advertised = Some((real.len() as u64, to_hex(&Sha256::digest(&real))));
    let mut evil = real.clone();
    evil[0] = b'X';
    release.installer = evil;
    let service = h.service("0.1.0", release.responses());
    service.check(ON).unwrap();
    assert_eq!(service.download(ON), Err(UpdateError::IntegrityFailed));
    assert!(h.staged_files().is_empty());

    // A stream larger than the signed size is refused.
    let mut release = Release::default();
    release.advertised = Some((10, to_hex(&Sha256::digest(&release.installer))));
    let service = h.service("0.1.0", release.responses());
    service.check(ON).unwrap();
    assert_eq!(service.download(ON), Err(UpdateError::IntegrityFailed));
    assert!(h.staged_files().is_empty());
}

#[test]
fn interrupted_downloads_leave_nothing_behind() {
    let h = Harness::new("interrupt");
    let transport = CountingTransport::with(Release::default().responses());
    *transport.interrupt_after.lock().unwrap() = Some(100);
    let service = h.service_with("0.1.0", transport);
    service.check(ON).unwrap();
    assert_eq!(service.download(ON), Err(UpdateError::DownloadFailed));
    assert!(h.staged_files().is_empty());

    // A .part left by a crash (power loss) is removed at the next start.
    std::fs::write(
        h.dir.join("updates").join(format!("{INSTALLER}.part")),
        b"partial",
    )
    .unwrap();
    let service = h.starting("0.1.0");
    assert_eq!(service.recover_on_startup(), RecoveryOutcome::Clean);
    assert!(h.staged_files().is_empty());
}

#[test]
fn authenticode_policy_matrix() {
    use AuthenticodeVerdict::{Invalid, Unavailable, Unsigned};
    use UpdateSigning::{Authenticode, None as NoSigning};
    /// (manifest signing, manifest thumbprint, running app, candidate, allowed, case)
    type Case = (
        UpdateSigning,
        Option<&'static str>,
        AuthenticodeVerdict,
        AuthenticodeVerdict,
        bool,
        &'static str,
    );
    let cases: &[Case] = &[
        (
            NoSigning,
            None,
            Unsigned,
            Unsigned,
            true,
            "unsigned app, unsigned update",
        ),
        (
            NoSigning,
            None,
            Unsigned,
            valid(THUMB_A),
            true,
            "unsigned app may move to signed",
        ),
        (
            NoSigning,
            None,
            Unsigned,
            Invalid,
            false,
            "broken signature",
        ),
        (
            NoSigning,
            None,
            Unsigned,
            Unavailable,
            false,
            "unknown signature state",
        ),
        (
            NoSigning,
            None,
            valid(THUMB_A),
            Unsigned,
            false,
            "signed app never downgrades",
        ),
        (
            NoSigning,
            None,
            valid(THUMB_A),
            valid(THUMB_B),
            false,
            "signed app never switches signer",
        ),
        (
            NoSigning,
            None,
            valid(THUMB_A),
            valid(THUMB_A),
            true,
            "same signer",
        ),
        (
            Authenticode,
            Some(THUMB_A),
            Unsigned,
            valid(THUMB_A),
            true,
            "manifest signer matches",
        ),
        (
            Authenticode,
            Some(THUMB_A),
            Unsigned,
            valid(THUMB_B),
            false,
            "wrong signer",
        ),
        (
            Authenticode,
            Some(THUMB_A),
            Unsigned,
            Unsigned,
            false,
            "manifest demands a signature",
        ),
        (
            Authenticode,
            Some(THUMB_A),
            valid(THUMB_A),
            valid(THUMB_A),
            true,
            "signed to signed",
        ),
        (
            Authenticode,
            None,
            Unsigned,
            valid(THUMB_A),
            false,
            "authenticode without thumbprint",
        ),
        (
            NoSigning,
            None,
            Invalid,
            Unsigned,
            false,
            "running app state broken",
        ),
        (
            NoSigning,
            None,
            Unavailable,
            Unsigned,
            false,
            "running app state unknown",
        ),
    ];
    for (signing, thumb, running, candidate, expected, name) in cases {
        assert_eq!(
            authenticode_allows(*signing, *thumb, running, candidate),
            *expected,
            "{name}"
        );
    }
}

#[test]
fn signature_policy_rejects_downgrades_at_download() {
    let h = Harness::new("downgrade");
    h.set_running(valid(THUMB_A));
    h.set_candidate(AuthenticodeVerdict::Unsigned);
    let service = h.service("0.1.0", Release::default().responses());
    service.check(ON).unwrap();
    assert_eq!(service.download(ON), Err(UpdateError::SignatureRejected));
    assert!(h.staged_files().is_empty());

    let h = Harness::new("wrongsigner");
    h.set_candidate(valid(THUMB_B));
    let release = Release {
        signing: "authenticode",
        thumbprint: Some(THUMB_A),
        ..Release::default()
    };
    let service = h.service("0.1.0", release.responses());
    service.check(ON).unwrap();
    assert_eq!(service.download(ON), Err(UpdateError::SignatureRejected));
    assert!(h.staged_files().is_empty());
}

fn staged_service(h: &Harness) -> UpdateService<CountingTransport> {
    let service = h.service("0.1.0", Release::default().responses());
    service.check(ON).unwrap();
    service.download(ON).unwrap();
    service
}

#[test]
fn install_asks_first_and_declining_changes_nothing() {
    let h = Harness::new("decline");
    let service = staged_service(&h);
    let asked = Mutex::new(Vec::new());
    let result = service.install(&|title, body| {
        asked
            .lock()
            .unwrap()
            .push((title.to_owned(), body.to_owned()));
        false
    });
    assert_eq!(result, Err(UpdateError::Declined));
    assert!(h.launch.launched.lock().unwrap().is_empty());
    assert_eq!(
        service.status().update,
        UpdateState::Verified {
            version: "0.2.0".into()
        }
    );
    let asked = asked.lock().unwrap();
    assert!(asked[0].1.contains("0.1.0") && asked[0].1.contains("0.2.0"));
}

#[test]
fn install_launches_the_locked_verified_installer() {
    let h = Harness::new("install");
    let service = staged_service(&h);
    assert_eq!(
        service.install(&|_, _| true).unwrap().update,
        UpdateState::Launched {
            version: "0.2.0".into()
        }
    );
    let launched = h.launch.launched.lock().unwrap().clone();
    assert_eq!(launched, vec![h.dir.join("updates").join(INSTALLER)]);
}

#[test]
fn install_rechecks_the_file_and_recovers_from_launch_failure() {
    let h = Harness::new("recheck");
    let service = staged_service(&h);
    // Swapped after download: rejected and removed, never launched.
    std::fs::write(h.dir.join("updates").join(INSTALLER), b"swapped").unwrap();
    assert_eq!(
        service.install(&|_, _| true),
        Err(UpdateError::IntegrityFailed)
    );
    assert!(h.launch.launched.lock().unwrap().is_empty());
    assert!(h.staged_files().is_empty());
    assert_eq!(
        service.install(&|_, _| true),
        Err(UpdateError::NothingToInstall)
    );

    let h = Harness::new("launchfail");
    let service = staged_service(&h);
    *h.launch.fail.lock().unwrap() = true;
    assert_eq!(
        service.install(&|_, _| true),
        Err(UpdateError::LaunchFailed)
    );
    assert_eq!(
        service.status().update,
        UpdateState::Verified {
            version: "0.2.0".into()
        }
    );
}

#[test]
fn startup_recovery_resolves_each_interrupted_state() {
    // Launched, and the new version is now running: success, staging cleaned.
    let h = Harness::new("recover-ok");
    staged_service(&h).install(&|_, _| true).unwrap();
    let after = h.starting("0.2.0");
    assert!(after.status().recovery_pending);
    assert_eq!(after.status().recovery, None);
    assert_eq!(
        after.recover_on_startup(),
        RecoveryOutcome::Updated {
            version: "0.2.0".into()
        }
    );
    assert!(h.staged_files().is_empty());
    assert_eq!(after.status().update, UpdateState::Idle);
    assert!(!after.status().recovery_pending);
    assert_eq!(
        after.status().recovery,
        Some(RecoveryOutcome::Updated {
            version: "0.2.0".into()
        })
    );
    assert_eq!(after.acknowledge_recovery().recovery, None);
    assert_eq!(after.status().recovery, None);

    // Launched, but the old version still runs (UAC declined, installer killed).
    let h = Harness::new("recover-int");
    staged_service(&h).install(&|_, _| true).unwrap();
    let after = h.starting("0.1.0");
    assert_eq!(
        after.recover_on_startup(),
        RecoveryOutcome::Interrupted {
            version: "0.2.0".into()
        }
    );
    assert_eq!(
        after.status().update,
        UpdateState::Interrupted {
            version: "0.2.0".into()
        }
    );
    assert_eq!(
        after.status().recovery,
        Some(RecoveryOutcome::Interrupted {
            version: "0.2.0".into()
        })
    );
    // Retry uses the kept, re-verified installer and resolves the notice.
    let retried = after.install(&|_, _| true).unwrap();
    assert_eq!(retried.recovery, None);
    assert_eq!(h.launch.launched.lock().unwrap().len(), 2);

    // Discarding an interrupted update also resolves the notice.
    let h = Harness::new("recover-int-discard");
    staged_service(&h).install(&|_, _| true).unwrap();
    let after = h.starting("0.1.0");
    after.recover_on_startup();
    let discarded = after.discard().unwrap();
    assert_eq!(discarded.update, UpdateState::Idle);
    assert_eq!(discarded.recovery, None);
    assert!(h.staged_files().is_empty());

    // Verified file corrupted while the app was closed: discarded.
    let h = Harness::new("recover-bad");
    drop(staged_service(&h));
    std::fs::write(h.dir.join("updates").join(INSTALLER), b"corrupt").unwrap();
    let after = h.starting("0.1.0");
    assert_eq!(after.recover_on_startup(), RecoveryOutcome::Discarded);
    assert!(h.staged_files().is_empty());
    assert_eq!(after.status().recovery, Some(RecoveryOutcome::Discarded));
    assert_eq!(after.discard().unwrap().recovery, None);

    // Unreadable state file falls back to idle and removes strays.
    let h = Harness::new("recover-junk");
    std::fs::create_dir_all(h.dir.join("updates")).unwrap();
    std::fs::write(h.dir.join("updates/state.json"), b"{not json").unwrap();
    std::fs::write(h.dir.join("updates").join(INSTALLER), b"stray").unwrap();
    std::fs::write(h.dir.join("updates/keep-me.txt"), b"unrelated").unwrap();
    let after = h.starting("0.1.0");
    assert_eq!(after.recover_on_startup(), RecoveryOutcome::Clean);
    assert_eq!(after.status().recovery, None, "clean is not a notice");
    assert!(!after.status().recovery_pending);
    assert!(!h.dir.join("updates").join(INSTALLER).exists());
    assert!(
        h.dir.join("updates/keep-me.txt").exists(),
        "only installer names are touched"
    );
}

#[test]
fn status_is_recovering_not_idle_until_startup_recovery_loads_disk_state() {
    let h = Harness::new("recovering");
    drop(staged_service(&h));
    let after = h.starting("0.1.0");
    // state.json holds Verified: before recovery, never report a plain Idle.
    assert_eq!(after.status().update, UpdateState::Recovering);
    // User operations wait for recovery instead of sweeping the staged file.
    assert_eq!(after.check(ON), Err(UpdateError::Busy));
    assert_eq!(after.discard(), Err(UpdateError::Busy));
    assert_eq!(h.staged_files(), vec![INSTALLER]);

    assert_eq!(after.recover_on_startup(), RecoveryOutcome::Clean);
    assert_eq!(
        after.status().update,
        UpdateState::Verified {
            version: "0.2.0".into()
        }
    );
}

#[test]
fn startup_recovery_waits_for_a_briefly_held_operation_lock() {
    let h = Harness::new("recover-lock");
    staged_service(&h).install(&|_, _| true).unwrap();
    let after = h.starting("0.1.0");
    let held = after.operation.lock().unwrap();
    std::thread::scope(|scope| {
        let recovery = scope.spawn(|| after.recover_on_startup());
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(after.status().update, UpdateState::Recovering);
        drop(held);
        assert_eq!(
            recovery.join().unwrap(),
            RecoveryOutcome::Interrupted {
                version: "0.2.0".into()
            }
        );
    });
    assert!(!after.status().recovery_pending);
    assert_eq!(
        after.status().update,
        UpdateState::Interrupted {
            version: "0.2.0".into()
        }
    );
}

#[test]
fn discard_removes_staged_files() {
    let h = Harness::new("discard");
    let service = staged_service(&h);
    assert_eq!(service.discard().unwrap().update, UpdateState::Idle);
    assert!(h.staged_files().is_empty());
}

#[test]
fn only_exact_installer_names_are_recognised() {
    assert!(is_installer_file("Supa-Diska-Klinah_0.2.0_x64-setup.exe"));
    assert!(is_installer_file(
        "Supa-Diska-Klinah_0.2.0_x64-setup.exe.part"
    ));
    for name in [
        "Supa-Diska-Klinah_0.2_x64-setup.exe",
        "Supa-Diska-Klinah_00.2.0_x64-setup.exe",
        "Supa-Diska-Klinah_0.2.0_x64-setup.exe.bak",
        "state.json",
        "..\\Supa-Diska-Klinah_0.2.0_x64-setup.exe",
    ] {
        assert!(!is_installer_file(name), "{name}");
    }
}

#[test]
fn the_embedded_key_is_readable() {
    // Until a release key is generated the marker yields NotConfigured.
    match embedded_verifier() {
        Ok(_) | Err(UpdateManifestError::NotConfigured) => {}
        Err(other) => panic!("embedded update key is malformed: {other}"),
    }
}

/// Release-acceptance rehearsal against the REAL Windows signature check
/// (`WinVerifyTrust` via `SystemProbe`) and real PE files. Ignored by default;
/// `scripts/acceptance-release.ps1` runs it with:
///   SDK_REHEARSAL_UNSIGNED = an unsigned candidate installer (the release under test)
///   SDK_REHEARSAL_SIGNED   = an embedded-Authenticode file from another publisher
///                            (default: Microsoft Edge), used as a real "wrong signer"
///                            and, byte-corrupted, as a real broken signature.
/// Only the installer launch is faked: nothing is installed.
mod rehearsal {
    use super::*;

    fn env_file(name: &str, default: Option<&str>) -> Vec<u8> {
        let path = std::env::var(name)
            .ok()
            .or_else(|| default.map(str::to_owned))
            .unwrap_or_else(|| panic!("{name} must point at a file for the rehearsal"));
        std::fs::read(&path).unwrap_or_else(|error| panic!("{name}={path}: {error}"))
    }

    fn unsigned() -> Vec<u8> {
        env_file("SDK_REHEARSAL_UNSIGNED", None)
    }

    fn signed() -> Vec<u8> {
        env_file(
            "SDK_REHEARSAL_SIGNED",
            Some(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
        )
    }

    /// Flips one byte inside the signed image (outside the PE header), so the
    /// embedded signature no longer matches: a genuinely broken signature.
    fn corrupted(mut bytes: Vec<u8>) -> Vec<u8> {
        let index = bytes.len() / 2;
        bytes[index] ^= 0xFF;
        bytes
    }

    /// A service after startup recovery, as the app runs it.
    fn service(
        h: &Harness,
        running_exe: &[u8],
        release: &Release,
    ) -> UpdateService<CountingTransport> {
        let service = starting(h, running_exe, release);
        service.recover_on_startup();
        service
    }

    /// A freshly started service whose startup recovery has not run yet.
    fn starting(
        h: &Harness,
        running_exe: &[u8],
        release: &Release,
    ) -> UpdateService<CountingTransport> {
        let exe = h.dir.join("running-app.exe");
        std::fs::write(&exe, running_exe).unwrap();
        UpdateService::new(UpdateServiceConfig {
            app_data: h.dir.clone(),
            running_version: "0.1.0".into(),
            running_exe: exe,
            transport: CountingTransport::with(release.responses()),
            verifier: Ok(verifier()),
            clock,
            probe: Box::new(SystemProbe),
            launcher: Box::new(FakeLauncher(Arc::clone(&h.launch))),
        })
        .unwrap()
    }

    fn real_thumbprint(bytes: &[u8], h: &Harness) -> String {
        let path = h.dir.join("probe-signed.exe");
        std::fs::write(&path, bytes).unwrap();
        let file = File::open(&path).unwrap();
        match SystemProbe.verdict(&path, &file) {
            AuthenticodeVerdict::Valid { thumbprint } => thumbprint,
            other => panic!("SDK_REHEARSAL_SIGNED is not validly signed: {other:?}"),
        }
    }

    /// Check, then download only when an update is offered (as the UI does).
    fn outcome(service: &UpdateService<CountingTransport>) -> Result<UpdateState, UpdateError> {
        match service.check(ON)?.update {
            UpdateState::Available { .. } => service.download(ON).map(|status| status.update),
            other => Ok(other),
        }
    }

    #[test]
    #[ignore = "release acceptance: needs real PE files (see module docs)"]
    fn real_signature_rehearsal() {
        let h = Harness::new("rehearsal");
        let (unsigned, signed) = (unsigned(), signed());
        let ms = real_thumbprint(&signed, &h);
        assert_eq!(
            SystemProbe.verdict(
                &h.dir.join("probe-signed.exe"),
                &File::open(h.dir.join("probe-signed.exe")).unwrap()
            ),
            valid(&ms)
        );
        let mut report = Vec::new();
        let mut case = |name: &str,
                        h: &Harness,
                        running: &[u8],
                        release: Release,
                        expected: Result<UpdateState, UpdateError>| {
            let service = service(h, running, &release);
            let actual = outcome(&service);
            assert_eq!(actual, expected, "{name}");
            report.push(format!("{name}: {actual:?}"));
            let _ = service.discard();
        };
        let verified = Ok(UpdateState::Verified {
            version: "0.2.0".into(),
        });

        // Today's path: unsigned app, unsigned release, SHA-256 is the root.
        case(
            "unsigned->unsigned accepted",
            &h,
            &unsigned,
            Release {
                installer: unsigned.clone(),
                ..Release::default()
            },
            verified.clone(),
        );
        // Real broken signature is rejected even while releases are unsigned.
        case(
            "broken signature rejected",
            &h,
            &unsigned,
            Release {
                installer: corrupted(signed.clone()),
                ..Release::default()
            },
            Err(UpdateError::SignatureRejected),
        );
        // Authenticode manifest naming our (test) thumbprint; file signed by someone else.
        case(
            "wrong signer rejected",
            &h,
            &unsigned,
            Release {
                installer: signed.clone(),
                signing: "authenticode",
                thumbprint: Some(THUMB_A),
                ..Release::default()
            },
            Err(UpdateError::SignatureRejected),
        );
        // Same real signer as the manifest: accepted (positive Authenticode path).
        let ms_static: &'static str = Box::leak(ms.clone().into_boxed_str());
        case(
            "matching signer accepted",
            &h,
            &unsigned,
            Release {
                installer: signed.clone(),
                signing: "authenticode",
                thumbprint: Some(ms_static),
                ..Release::default()
            },
            verified.clone(),
        );
        // A signed install never accepts an unsigned update, whatever the manifest says.
        case(
            "signed->unsigned downgrade rejected",
            &h,
            &signed,
            Release {
                installer: unsigned.clone(),
                ..Release::default()
            },
            Err(UpdateError::SignatureRejected),
        );
        // Tampered installer: size/SHA-256 no longer match the signed manifest.
        let mut tampered = unsigned.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        case(
            "tampered installer rejected",
            &h,
            &unsigned,
            Release {
                installer: tampered,
                advertised: Some((unsigned.len() as u64, to_hex(&Sha256::digest(&unsigned)))),
                ..Release::default()
            },
            Err(UpdateError::IntegrityFailed),
        );
        // Rollback: an older or equal version is never offered.
        case(
            "rollback not offered",
            &h,
            &unsigned,
            Release {
                version: "0.1.0",
                installer: unsigned.clone(),
                ..Release::default()
            },
            Ok(UpdateState::UpToDate),
        );
        assert!(
            h.staged_files().is_empty(),
            "rejected installers must be deleted: {:?}",
            h.staged_files()
        );

        // Tampered manifest: signature over different bytes.
        let release = Release {
            installer: unsigned.clone(),
            ..Release::default()
        };
        let mut manifest = release.manifest();
        let signature = release.signature(&manifest);
        let at = manifest.iter().position(|&b| b == b'1').unwrap();
        manifest[at] = b'2';
        let exe = h.dir.join("running-app.exe");
        let tampered_manifest = UpdateService::new(UpdateServiceConfig {
            app_data: h.dir.clone(),
            running_version: "0.1.0".into(),
            running_exe: exe.clone(),
            transport: CountingTransport::with(vec![Ok(manifest), Ok(signature)]),
            verifier: Ok(verifier()),
            clock,
            probe: Box::new(SystemProbe),
            launcher: Box::new(FakeLauncher(Arc::clone(&h.launch))),
        })
        .unwrap();
        assert_eq!(tampered_manifest.check(ON), Err(UpdateError::BadManifest));
        report.push("tampered manifest: BadManifest".into());

        // Interrupted download: nothing staged; startup recovery leaves a clean state.
        let interrupted = UpdateService::new(UpdateServiceConfig {
            app_data: h.dir.clone(),
            running_version: "0.1.0".into(),
            running_exe: exe.clone(),
            transport: CountingTransport::with({
                let mut responses = release.responses();
                responses[2] = Err(NetError::Unreachable);
                responses
            }),
            verifier: Ok(verifier()),
            clock,
            probe: Box::new(SystemProbe),
            launcher: Box::new(FakeLauncher(Arc::clone(&h.launch))),
        })
        .unwrap();
        interrupted.recover_on_startup();
        assert_eq!(outcome(&interrupted), Err(UpdateError::DownloadFailed));
        assert_eq!(interrupted.recover_on_startup(), RecoveryOutcome::Clean);
        assert!(h.staged_files().is_empty());
        report.push("interrupted download: DownloadFailed, recovery Clean".into());

        // Declined confirmation ("UAC declined" at the app's own prompt): nothing launched.
        let declined = service(&h, &unsigned, &release);
        outcome(&declined).unwrap();
        assert_eq!(declined.install(&|_, _| false), Err(UpdateError::Declined));
        assert!(h.launch.launched.lock().unwrap().is_empty());
        // Installer started but did not finish (e.g. UAC cancelled in the installer):
        // the next start reports Interrupted and keeps the re-verified installer.
        declined.install(&|_, _| true).unwrap();
        let restarted = starting(&h, &unsigned, &Release::default());
        assert_eq!(
            restarted.recover_on_startup(),
            RecoveryOutcome::Interrupted {
                version: "0.2.0".into()
            }
        );
        assert_eq!(h.staged_files(), vec![INSTALLER.to_owned()]);
        report.push(
            "declined: Declined; installer cancelled: Interrupted (kept, re-verified)".into(),
        );

        println!("REHEARSAL RESULTS\n{}", report.join("\n"));
    }
}

#[test]
fn status_serializes_recovery_as_optional_camel_case() {
    let h = Harness::new("recover-json");
    let service = h.starting("0.1.0");
    let pending = serde_json::to_value(service.status()).unwrap();
    assert_eq!(pending["recoveryPending"], true);
    assert_eq!(
        pending["update"],
        serde_json::json!({ "state": "recovering" })
    );
    assert!(pending.get("recovery").is_none());

    service.recover_on_startup();
    let clean = serde_json::to_value(service.status()).unwrap();
    assert!(clean.get("recovery").is_none());
    assert!(clean.get("recoveryPending").is_none());

    staged_service(&h).install(&|_, _| true).unwrap();
    let after = h.starting("0.1.0");
    after.recover_on_startup();
    let json = serde_json::to_value(after.status()).unwrap();
    assert_eq!(
        json["recovery"],
        serde_json::json!({ "outcome": "interrupted", "version": "0.2.0" })
    );
}
