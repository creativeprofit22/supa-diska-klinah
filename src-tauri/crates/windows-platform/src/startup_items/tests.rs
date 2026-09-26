use std::{collections::HashMap, sync::Mutex};

use cleanup_core::system_change::{
    EntryName, PriorState, RiskLevel, StartupEntryRef, StartupLocation, StartupScope, SystemChange,
    UnsupportedReason,
};

use super::*;
use crate::{
    os_info::{Edition, OsFacts, SUPPORTED_BUILDS},
    security::system_changes::HelperChange,
};

type ApprovedKey = (String, String, String);

fn hk(hive: Hive) -> String {
    format!("{hive:?}")
}

type RecordedWrite = (Hive, String, String, Vec<u8>);

#[derive(Default)]
struct FakeStore {
    registry: HashMap<(String, &'static str), Vec<SourceEntry>>,
    folders: HashMap<StartupScope, Vec<SourceEntry>>,
    approved: Mutex<HashMap<ApprovedKey, Vec<u8>>>,
    tasks: Vec<LogonTask>,
    fail: Option<AdapterError>,
    writes: Mutex<Vec<RecordedWrite>>,
}

impl FakeStore {
    fn with_run(mut self, hive: Hive, path: &'static str, name: &str) -> Self {
        self.registry
            .entry((hk(hive), path))
            .or_default()
            .push(SourceEntry {
                name: name.into(),
                command: format!(r"C:\Apps\{name}.exe --tray"),
            });
        self
    }

    fn with_file(mut self, scope: StartupScope, name: &str) -> Self {
        self.folders.entry(scope).or_default().push(SourceEntry {
            name: name.into(),
            command: format!(r"C:\Startup\{name}"),
        });
        self
    }

    fn with_approved(self, hive: Hive, path: &str, name: &str, data: &[u8]) -> Self {
        self.approved
            .lock()
            .unwrap()
            .insert((hk(hive), path.into(), name.into()), data.to_vec());
        self
    }

    fn failing(mut self, error: AdapterError) -> Self {
        self.fail = Some(error);
        self
    }

    fn write_count(&self) -> usize {
        self.writes.lock().unwrap().len()
    }
}

impl StartupReader for FakeStore {
    fn registry_values(&self, hive: Hive, path: &str) -> Result<Vec<SourceEntry>, AdapterError> {
        if let Some(error) = self.fail {
            return Err(error);
        }
        Ok(self
            .registry
            .iter()
            .find(|((h, p), _)| *h == hk(hive) && *p == path)
            .map(|(_, v)| v.clone())
            .unwrap_or_default())
    }

    fn folder_files(&self, scope: StartupScope) -> Result<Vec<SourceEntry>, AdapterError> {
        if let Some(error) = self.fail {
            return Err(error);
        }
        Ok(self.folders.get(&scope).cloned().unwrap_or_default())
    }

    fn approved(
        &self,
        hive: Hive,
        path: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>, AdapterError> {
        Ok(self
            .approved
            .lock()
            .unwrap()
            .get(&(hk(hive), path.into(), name.into()))
            .cloned())
    }

    fn logon_tasks(&self) -> Result<Vec<LogonTask>, AdapterError> {
        if let Some(error) = self.fail {
            return Err(error);
        }
        Ok(self.tasks.clone())
    }
}

impl StartupWriter for FakeStore {
    fn set_approved(
        &self,
        hive: Hive,
        path: &str,
        name: &str,
        data: &[u8],
    ) -> Result<(), AdapterError> {
        self.writes
            .lock()
            .unwrap()
            .push((hive, path.into(), name.into(), data.to_vec()));
        self.approved
            .lock()
            .unwrap()
            .insert((hk(hive), path.into(), name.into()), data.to_vec());
        Ok(())
    }
}

impl StartupReader for &FakeStore {
    fn registry_values(&self, hive: Hive, path: &str) -> Result<Vec<SourceEntry>, AdapterError> {
        (*self).registry_values(hive, path)
    }
    fn folder_files(&self, scope: StartupScope) -> Result<Vec<SourceEntry>, AdapterError> {
        (*self).folder_files(scope)
    }
    fn approved(
        &self,
        hive: Hive,
        path: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>, AdapterError> {
        (*self).approved(hive, path, name)
    }
    fn logon_tasks(&self) -> Result<Vec<LogonTask>, AdapterError> {
        (*self).logon_tasks()
    }
}

impl StartupWriter for &FakeStore {
    fn set_approved(
        &self,
        hive: Hive,
        path: &str,
        name: &str,
        data: &[u8],
    ) -> Result<(), AdapterError> {
        (*self).set_approved(hive, path, name, data)
    }
}

fn entry(scope: StartupScope, location: StartupLocation, name: &str) -> StartupEntryRef {
    StartupEntryRef {
        scope,
        location,
        name: EntryName::parse(name).unwrap(),
    }
}

fn change(entry: StartupEntryRef, enabled: bool) -> SystemChange {
    SystemChange::SetStartupEntry { entry, enabled }
}

fn adapter(store: &FakeStore) -> StartupAdapter<&FakeStore, &FakeStore> {
    StartupAdapter::with(store, store)
}

fn user_run(name: &str) -> StartupEntryRef {
    entry(StartupScope::User, StartupLocation::Run, name)
}

#[test]
fn decodes_startup_approved_bytes() {
    assert!(decode_approved(None));
    assert!(decode_approved(Some(&[])));
    assert!(decode_approved(Some(&[0x02, 0, 0, 0])));
    assert!(decode_approved(Some(&[0x06, 0, 0, 0])));
    assert!(!decode_approved(Some(&[0x03, 0, 0, 0])));
    assert!(!decode_approved(Some(&[0x07, 0, 0, 0])));
}

#[test]
fn encodes_startup_approved_bytes() {
    assert_eq!(
        encode_approved(true, 0x1122_3344_5566_7788),
        [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    let disabled = encode_approved(false, 0x1122_3344_5566_7788);
    assert_eq!(disabled[..4], [3, 0, 0, 0]);
    assert_eq!(disabled[4..], 0x1122_3344_5566_7788_u64.to_le_bytes());
    assert!(!decode_approved(Some(&disabled)));
    assert!(decode_approved(Some(&encode_approved(true, 5))));
    assert!(now_filetime() > 130_000_000_000_000_000);
}

#[test]
fn absent_approved_value_means_enabled() {
    let store = FakeStore::default().with_run(Hive::CurrentUser, RUN_KEY, "Tray");
    let state = adapter(&store)
        .observe(&change(user_run("Tray"), false))
        .unwrap();
    assert_eq!(state, PriorState::Enabled { enabled: true });
}

#[test]
fn observes_disabled_entries_per_location() {
    let store = FakeStore::default()
        .with_run(Hive::LocalMachine, RUN32_KEY, "Legacy")
        .with_file(StartupScope::User, "Notes.lnk")
        .with_approved(
            Hive::LocalMachine,
            APPROVED_RUN32_KEY,
            "Legacy",
            &[7, 0, 0, 0],
        )
        .with_approved(
            Hive::CurrentUser,
            APPROVED_FOLDER_KEY,
            "Notes.lnk",
            &[3; 12],
        );
    let adapter = adapter(&store);
    let legacy = entry(StartupScope::Machine, StartupLocation::Run32, "Legacy");
    let notes = entry(
        StartupScope::User,
        StartupLocation::StartupFolder,
        "Notes.lnk",
    );
    assert_eq!(
        adapter.observe(&change(legacy, true)).unwrap(),
        PriorState::Enabled { enabled: false }
    );
    assert_eq!(
        adapter.observe(&change(notes, true)).unwrap(),
        PriorState::Enabled { enabled: false }
    );
}

#[test]
fn absent_or_invalid_entries_fail_closed() {
    let store = FakeStore::default().with_run(Hive::CurrentUser, RUN_KEY, "Tray");
    let adapter = adapter(&store);
    let not_present = Err(AdapterError::Unsupported(UnsupportedReason::NotPresent));
    assert_eq!(
        adapter.observe(&change(user_run("Missing"), false)),
        not_present
    );
    assert_eq!(
        adapter.apply(&change(user_run("Missing"), false)),
        Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
    );
    let user_run32 = entry(StartupScope::User, StartupLocation::Run32, "Tray");
    assert_eq!(
        adapter.observe(&change(user_run32.clone(), false)),
        not_present
    );
    assert_eq!(
        adapter.describe(&change(user_run32, false)).err(),
        Some(AdapterError::Unsupported(UnsupportedReason::NotPresent))
    );
    // HKLM\Run holds no such value even though HKCU\Run does.
    let machine = entry(StartupScope::Machine, StartupLocation::Run, "Tray");
    assert_eq!(
        elevated::observe_with(&store, &helper(machine, false)),
        not_present
    );
    assert_eq!(store.write_count(), 0);
}

#[test]
fn disable_then_enable_writes_only_startup_approved() {
    let store = FakeStore::default().with_run(Hive::CurrentUser, RUN_KEY, "Tray");
    let adapter = adapter(&store);
    adapter.apply(&change(user_run("Tray"), false)).unwrap();
    adapter.apply(&change(user_run("Tray"), true)).unwrap();
    let writes = store.writes.lock().unwrap().clone();
    assert_eq!(writes.len(), 2);
    for (hive, path, name, data) in &writes {
        assert_eq!(*hive, Hive::CurrentUser);
        assert_eq!(path, APPROVED_RUN_KEY);
        assert_eq!(name, "Tray");
        assert_eq!(data.len(), 12);
    }
    assert_eq!(writes[0].3[0], 0x03);
    assert_eq!(writes[1].3, vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    // The Run value itself is untouched.
    assert_eq!(store.registry[&(hk(Hive::CurrentUser), RUN_KEY)].len(), 1);
}

#[test]
fn reapply_is_idempotent() {
    let store = FakeStore::default()
        .with_file(StartupScope::User, "Notes.lnk")
        .with_approved(
            Hive::CurrentUser,
            APPROVED_FOLDER_KEY,
            "Notes.lnk",
            &[6; 12],
        );
    let adapter = adapter(&store);
    let enable = change(
        entry(
            StartupScope::User,
            StartupLocation::StartupFolder,
            "Notes.lnk",
        ),
        true,
    );
    let state = adapter.observe(&enable).unwrap();
    assert_eq!(enable.is_satisfied_by(&state), Some(true));
    adapter.apply(&enable).unwrap();
    assert_eq!(store.write_count(), 0);

    let disable = change(
        entry(
            StartupScope::User,
            StartupLocation::StartupFolder,
            "Notes.lnk",
        ),
        false,
    );
    adapter.apply(&disable).unwrap();
    adapter.apply(&disable).unwrap();
    assert_eq!(store.write_count(), 1);
    assert_eq!(
        disable.is_satisfied_by(&adapter.observe(&disable).unwrap()),
        Some(true)
    );
}

#[test]
fn inverse_restores_prior_state() {
    let store = FakeStore::default().with_run(Hive::CurrentUser, RUN_KEY, "Tray");
    let adapter = adapter(&store);
    let disable = change(user_run("Tray"), false);
    let prior = adapter.observe(&disable).unwrap();
    adapter.apply(&disable).unwrap();
    let inverse = disable.inverse(&prior).unwrap().unwrap();
    assert_eq!(inverse, change(user_run("Tray"), true));
    adapter.apply(&inverse).unwrap();
    assert_eq!(adapter.observe(&disable).unwrap(), prior);
}

fn helper(entry: StartupEntryRef, enabled: bool) -> HelperChange {
    HelperChange::from_system_change(&change(entry, enabled)).unwrap()
}

#[test]
fn machine_scope_is_never_written_by_apply() {
    let store = FakeStore::default().with_run(Hive::LocalMachine, RUN_KEY, "Agent");
    let adapter = adapter(&store);
    let machine = change(
        entry(StartupScope::Machine, StartupLocation::Run, "Agent"),
        false,
    );
    assert_eq!(adapter.apply(&machine), Err(AdapterError::Failed));
    assert_eq!(store.write_count(), 0);
    assert_eq!(adapter.describe(&machine).unwrap().risk, RiskLevel::Medium);
    assert_eq!(
        adapter.observe(&machine).unwrap(),
        PriorState::Enabled { enabled: true }
    );
}

#[test]
fn elevated_helper_toggles_machine_entries_only() {
    let store = FakeStore::default().with_run(Hive::LocalMachine, RUN_KEY, "Agent");
    let machine = entry(StartupScope::Machine, StartupLocation::Run, "Agent");
    let request = helper(machine, false);
    assert_eq!(
        elevated::observe_with(&store, &request).unwrap(),
        PriorState::Enabled { enabled: true }
    );
    elevated::apply_with(&store, &store, &request).unwrap();
    let writes = store.writes.lock().unwrap().clone();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, Hive::LocalMachine);
    assert_eq!(writes[0].1, APPROVED_RUN_KEY);
    assert_eq!(
        elevated::observe_with(&store, &request).unwrap(),
        PriorState::Enabled { enabled: false }
    );
    let other = HelperChange::SetHibernation { enabled: true };
    assert_eq!(elevated::observe(&other), Err(AdapterError::Failed));
    assert_eq!(elevated::apply(&other), Err(AdapterError::Failed));
}

#[test]
fn access_denied_and_unavailable_api_propagate() {
    for error in [
        AdapterError::Denied,
        AdapterError::Unsupported(UnsupportedReason::ApiUnavailable),
    ] {
        let store = FakeStore::default()
            .with_run(Hive::CurrentUser, RUN_KEY, "Tray")
            .failing(error);
        let adapter = adapter(&store);
        assert_eq!(
            adapter.observe(&change(user_run("Tray"), false)),
            Err(error)
        );
        assert_eq!(adapter.apply(&change(user_run("Tray"), false)), Err(error));
        assert!(list_with(&store).is_empty());
    }
}

#[test]
fn behavior_is_identical_on_every_supported_build() {
    for build in SUPPORTED_BUILDS {
        let facts = OsFacts::fixture(build, Edition::Home, false);
        assert_eq!(facts.build, build);
        let store = FakeStore::default().with_run(Hive::CurrentUser, RUN_KEY, "Tray");
        let adapter = adapter(&store);
        adapter.apply(&change(user_run("Tray"), false)).unwrap();
        assert_eq!(
            adapter.observe(&change(user_run("Tray"), false)).unwrap(),
            PriorState::Enabled { enabled: false }
        );
    }
}

#[test]
fn inventory_lists_toggleable_and_read_only_kinds() {
    let mut store = FakeStore::default()
        .with_run(Hive::CurrentUser, RUN_KEY, "Tray")
        .with_run(Hive::LocalMachine, RUN32_KEY, "Legacy")
        .with_run(Hive::CurrentUser, RUN_ONCE_KEY, "Setup")
        .with_file(StartupScope::Machine, "Shared.lnk")
        .with_approved(Hive::CurrentUser, APPROVED_RUN_KEY, "Tray", &[3; 12]);
    store.registry.insert(
        (hk(Hive::LocalMachine), RUN_KEY),
        vec![SourceEntry {
            name: "Long".into(),
            command: "x".repeat(5000),
        }],
    );
    store.tasks.push(LogonTask {
        path: r"\Vendor\Updater".into(),
        enabled: false,
    });
    let items = list_with(&store);
    let find = |name: &str| items.iter().find(|item| item.name == name).unwrap();

    let tray = find("Tray");
    assert!(!tray.enabled && tray.toggleable);
    assert_eq!(tray.location, Some(StartupLocation::Run));
    assert_eq!(find("Legacy").location, Some(StartupLocation::Run32));
    assert_eq!(find("Shared.lnk").scope, StartupScope::Machine);
    assert_eq!(find("Long").command.chars().count(), MAX_COMMAND_CHARS);
    let run_once = find("Setup");
    assert!(!run_once.toggleable);
    assert_eq!(run_once.source, StartupSource::RunOnce);
    let task = find(r"\Vendor\Updater");
    assert!(!task.toggleable && !task.enabled);
    assert_eq!(task.source, StartupSource::LogonTask);
    assert_eq!(items.len(), 6);
}

#[test]
fn live_read_only_listing_succeeds() {
    let items = list_startup_items();
    assert!(items.len() <= MAX_ITEMS);
    for item in &items {
        assert!(item.command.chars().count() <= MAX_COMMAND_CHARS);
        assert!(item.name.chars().count() <= MAX_NAME_CHARS);
        assert_eq!(item.toggleable, item.location.is_some() && item.toggleable);
    }
    // Every live toggleable entry observes without error.
    let adapter = StartupAdapter::new();
    for item in items.iter().filter(|item| item.toggleable).take(16) {
        let Some(location) = item.location else {
            continue;
        };
        let observed = adapter.observe(&change(entry(item.scope, location, &item.name), true));
        assert!(
            matches!(observed, Ok(PriorState::Enabled { .. })),
            "{observed:?}"
        );
    }
    // Logon tasks enumerate through COM without error.
    WindowsStartupStore.logon_tasks().unwrap();
}
