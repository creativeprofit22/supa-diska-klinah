use std::{collections::BTreeMap, ffi::OsString, sync::Mutex};

use cleanup_core::system_change::{
    PriorState, ScheduleCadence, ScheduleId, SystemChange, UnsupportedReason, Weekday,
};

use super::*;

const EXE: &str = r"C:\Program Files\Supa Diska Klinah\supa-diska-klinah.exe";
const ID: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

fn today() -> LocalDate {
    LocalDate {
        year: 2026,
        month: 9,
        day: 3,
    }
}

fn id() -> ScheduleId {
    ScheduleId::parse(ID).unwrap()
}

#[derive(Default)]
struct FakeTasks {
    tasks: Mutex<BTreeMap<String, RawTask>>,
    registrations: Mutex<Vec<String>>,
    deletes: Mutex<usize>,
    fail: Option<AdapterError>,
}

impl FakeTasks {
    fn failing(error: AdapterError) -> Self {
        Self {
            fail: Some(error),
            ..Self::default()
        }
    }

    fn insert(&self, task: RawTask) {
        self.tasks.lock().unwrap().insert(task.name.clone(), task);
    }

    fn check(&self) -> Result<(), AdapterError> {
        self.fail.map_or(Ok(()), Err)
    }
}

impl TaskService for FakeTasks {
    fn get(&self, name: &str) -> Result<Option<RawTask>, AdapterError> {
        self.check()?;
        Ok(self.tasks.lock().unwrap().get(name).cloned())
    }

    fn list(&self) -> Result<Vec<RawTask>, AdapterError> {
        self.check()?;
        Ok(self.tasks.lock().unwrap().values().cloned().collect())
    }

    fn register(&self, name: &str, spec: &TaskSpec) -> Result<(), AdapterError> {
        self.check()?;
        self.registrations.lock().unwrap().push(name.to_owned());
        self.insert(RawTask {
            name: name.to_owned(),
            actions: vec![RawAction::Exec {
                path: spec.executable.clone(),
                arguments: spec.arguments.clone(),
            }],
            triggers: vec![spec.trigger.clone()],
            enabled: true,
            last_run: None,
            next_run: Some("2026-09-04T03:00:00".to_owned()),
        });
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), AdapterError> {
        self.check()?;
        *self.deletes.lock().unwrap() += 1;
        self.tasks.lock().unwrap().remove(name);
        Ok(())
    }
}

fn adapter(service: FakeTasks) -> SchedulerAdapter<FakeTasks> {
    SchedulerAdapter::with_service(service, EXE, today)
}

fn upsert(cadence: ScheduleCadence) -> SystemChange {
    SystemChange::UpsertScanSchedule {
        schedule_id: id(),
        cadence,
    }
}

fn remove() -> SystemChange {
    SystemChange::RemoveScanSchedule { schedule_id: id() }
}

fn our_task(path: &str, arguments: &str, trigger: RawTrigger) -> RawTask {
    RawTask {
        name: NAMING_SCHEME.task_name(&id()),
        actions: vec![RawAction::Exec {
            path: path.to_owned(),
            arguments: arguments.to_owned(),
        }],
        triggers: vec![trigger],
        enabled: true,
        last_run: None,
        next_run: None,
    }
}

fn daily_trigger(boundary: &str) -> RawTrigger {
    RawTrigger::Daily {
        days_interval: 1,
        start_boundary: boundary.to_owned(),
    }
}

const ALL_DAYS: [Weekday; 7] = [
    Weekday::Monday,
    Weekday::Tuesday,
    Weekday::Wednesday,
    Weekday::Thursday,
    Weekday::Friday,
    Weekday::Saturday,
    Weekday::Sunday,
];

#[test]
fn naming_scheme_round_trips_and_rejects_others() {
    assert_eq!(NAMING_SCHEME.folder, "\\SupaDiskaKlinah");
    let name = NAMING_SCHEME.task_name(&id());
    assert_eq!(name, format!("scan-{ID}"));
    assert_eq!(NAMING_SCHEME.parse_task_name(&name), Some(id()));
    assert_eq!(
        NAMING_SCHEME.parse_task_name(&format!("scan-{}", ID.to_uppercase())),
        None
    );
    assert_eq!(NAMING_SCHEME.parse_task_name("scan-"), None);
    assert_eq!(NAMING_SCHEME.parse_task_name(&format!("other-{ID}")), None);
}

#[test]
fn upsert_creates_then_updates_idempotently() {
    let adapter = adapter(FakeTasks::default());
    let daily = ScheduleCadence::Daily {
        hour: 2,
        minute: 30,
    };
    let weekly = ScheduleCadence::Weekly {
        day: Weekday::Friday,
        hour: 23,
        minute: 59,
    };

    let prior = adapter.observe(&upsert(daily)).unwrap();
    assert_eq!(prior, PriorState::Schedule { cadence: None });
    assert_eq!(upsert(daily).is_satisfied_by(&prior), Some(false));

    adapter.apply(&upsert(daily)).unwrap();
    let after = adapter.observe(&upsert(daily)).unwrap();
    assert_eq!(
        after,
        PriorState::Schedule {
            cadence: Some(daily)
        }
    );
    assert_eq!(upsert(daily).is_satisfied_by(&after), Some(true));

    adapter.apply(&upsert(daily)).unwrap();
    adapter.apply(&upsert(weekly)).unwrap();
    assert_eq!(
        adapter.observe(&upsert(weekly)).unwrap(),
        PriorState::Schedule {
            cadence: Some(weekly)
        }
    );
    let service = adapter.service();
    assert_eq!(service.tasks.lock().unwrap().len(), 1);
    assert_eq!(
        *service.registrations.lock().unwrap(),
        vec![NAMING_SCHEME.task_name(&id()); 3]
    );
}

#[test]
fn registered_spec_matches_contract() {
    let spec = TaskSpec::new(
        &id(),
        ScheduleCadence::Weekly {
            day: Weekday::Sunday,
            hour: 3,
            minute: 0,
        },
        EXE,
        today(),
    );
    assert_eq!(spec.executable, EXE);
    assert_eq!(spec.arguments, format!("--scheduled-scan {ID}"));
    assert_eq!(
        spec.trigger,
        RawTrigger::Weekly {
            days_of_week: 0x01,
            weeks_interval: 1,
            start_boundary: "2026-09-03T03:00:00".to_owned(),
        }
    );
}

#[test]
fn remove_twice_is_idempotent() {
    let adapter = adapter(FakeTasks::default());
    let cadence = ScheduleCadence::Daily { hour: 1, minute: 0 };
    adapter.apply(&upsert(cadence)).unwrap();
    assert_eq!(
        adapter.observe(&remove()).unwrap(),
        PriorState::Schedule {
            cadence: Some(cadence)
        }
    );
    adapter.apply(&remove()).unwrap();
    let prior = adapter.observe(&remove()).unwrap();
    assert_eq!(prior, PriorState::Schedule { cadence: None });
    assert_eq!(remove().is_satisfied_by(&prior), Some(true));
    adapter.apply(&remove()).unwrap();
    assert_eq!(*adapter.service().deletes.lock().unwrap(), 1);
}

#[test]
fn foreign_task_in_our_scheme_is_refused() {
    let args = format!("--scheduled-scan {ID}");
    let shapes = [
        RawTask {
            actions: vec![RawAction::Other],
            ..our_task(EXE, "", daily_trigger("2026-09-03T02:00:00"))
        },
        our_task(EXE, &args, RawTrigger::Other),
        our_task(
            EXE,
            &args,
            RawTrigger::Daily {
                days_interval: 2,
                start_boundary: "2026-09-03T02:00:00".to_owned(),
            },
        ),
        our_task(
            EXE,
            &args,
            RawTrigger::Weekly {
                days_of_week: 0x03,
                weeks_interval: 1,
                start_boundary: "2026-09-03T02:00:00".to_owned(),
            },
        ),
        our_task(EXE, &args, daily_trigger("2026-09-03T02:00:30")),
        RawTask {
            triggers: vec![
                daily_trigger("2026-09-03T02:00:00"),
                daily_trigger("2026-09-03T03:00:00"),
            ],
            ..our_task(EXE, &args, RawTrigger::Other)
        },
        our_task(
            r"C:\evil.exe",
            "/c whatever",
            daily_trigger("2026-09-03T02:00:00"),
        ),
    ];
    for task in shapes {
        let service = FakeTasks::default();
        service.insert(task.clone());
        let adapter = adapter(service);
        let change = upsert(ScheduleCadence::Daily { hour: 4, minute: 0 });
        assert_eq!(
            adapter.observe(&change),
            Err(AdapterError::Unsupported(UnsupportedReason::NotPresent)),
            "{task:?}"
        );
        assert_eq!(
            adapter.apply(&change),
            Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
        );
        assert!(adapter.service().registrations.lock().unwrap().is_empty());
        assert_eq!(adapter.service().tasks.lock().unwrap()[&task.name], task);
    }
}

#[test]
fn orphans_are_classified_listed_and_removable() {
    let service = FakeTasks::default();
    service.insert(our_task(
        r"D:\old\supa-diska-klinah.exe",
        &format!("--scheduled-scan {ID}"),
        daily_trigger("2026-09-03T02:00:00"),
    ));
    let other = ScheduleId::parse("11111111-2222-3333-4444-555555555555").unwrap();
    service.insert(RawTask {
        name: NAMING_SCHEME.task_name(&other),
        ..our_task(
            EXE,
            "--scheduled-scan",
            daily_trigger("2026-09-03T05:00:00"),
        )
    });
    let current = ScheduleId::parse("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap();
    service.insert(RawTask {
        name: NAMING_SCHEME.task_name(&current),
        ..our_task(
            &EXE.to_uppercase(),
            &scheduled_scan_arguments(&current),
            daily_trigger("2026-09-03T06:00:00"),
        )
    });
    let unrecognized = ScheduleId::parse("99999999-2222-3333-4444-555555555555").unwrap();
    service.insert(RawTask {
        name: NAMING_SCHEME.task_name(&unrecognized),
        ..our_task(EXE, "", RawTrigger::Other)
    });
    service.insert(RawTask {
        name: "SomeoneElse".to_owned(),
        ..our_task(EXE, "", RawTrigger::Other)
    });

    let listed = list_scan_schedules_with(&service, EXE).unwrap();
    let summary: Vec<_> = listed
        .iter()
        .map(|item| (item.id.to_string(), item.orphaned, item.orphan_reason))
        .collect();
    assert_eq!(
        summary,
        vec![
            (ID.to_owned(), true, Some(OrphanReason::ForeignExecutable)),
            (
                other.to_string(),
                true,
                Some(OrphanReason::MalformedArguments)
            ),
            (
                unrecognized.to_string(),
                true,
                Some(OrphanReason::UnrecognizedDefinition)
            ),
            (current.to_string(), false, None),
        ]
    );
    assert_eq!(
        listed[1].cadence,
        Some(ScheduleCadence::Daily { hour: 5, minute: 0 })
    );
    assert_eq!(listed[2].cadence, None);
    let json = serde_json::to_value(&listed[0]).unwrap();
    assert_eq!(json["orphanReason"], "foreignExecutable");
    assert!(json.get("nextRun").is_some() && json.get("lastRun").is_some());

    let adapter = adapter(service);
    // Orphans are never upserted over, but may be removed.
    assert_eq!(
        adapter.observe(&upsert(ScheduleCadence::Daily { hour: 2, minute: 0 })),
        Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
    );
    assert_eq!(
        adapter.observe(&remove()).unwrap(),
        PriorState::Schedule {
            cadence: Some(ScheduleCadence::Daily { hour: 2, minute: 0 })
        }
    );
    adapter.apply(&remove()).unwrap();
    assert!(
        adapter
            .service()
            .get(&NAMING_SCHEME.task_name(&id()))
            .unwrap()
            .is_none()
    );
    // Unrecognized definitions are never removed.
    let change = SystemChange::RemoveScanSchedule {
        schedule_id: unrecognized,
    };
    assert_eq!(
        adapter.apply(&change),
        Err(AdapterError::Unsupported(UnsupportedReason::NotPresent))
    );
}

#[test]
fn cadence_round_trips_for_all_weekdays_and_edge_times() {
    let times = [(0, 0), (0, 59), (12, 0), (23, 0), (23, 59)];
    let mut cadences = Vec::new();
    for (hour, minute) in times {
        cadences.push(ScheduleCadence::Daily { hour, minute });
        for day in ALL_DAYS {
            cadences.push(ScheduleCadence::Weekly { day, hour, minute });
        }
    }
    let adapter = adapter(FakeTasks::default());
    for cadence in cadences {
        let spec = TaskSpec::new(&id(), cadence, EXE, today());
        assert_eq!(decode_trigger(&spec.trigger), Some(cadence));
        adapter.apply(&upsert(cadence)).unwrap();
        assert_eq!(
            adapter.observe(&upsert(cadence)).unwrap(),
            PriorState::Schedule {
                cadence: Some(cadence)
            }
        );
    }
    let bits: Vec<_> = ALL_DAYS.into_iter().map(weekday_bit).collect();
    assert_eq!(bits, [0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x01]);
}

#[test]
fn invalid_cadence_is_rejected_before_writing() {
    let adapter = adapter(FakeTasks::default());
    let change = upsert(ScheduleCadence::Daily {
        hour: 24,
        minute: 0,
    });
    assert!(adapter.apply(&change).is_err());
    assert!(adapter.describe(&change).is_err());
    assert!(adapter.service().registrations.lock().unwrap().is_empty());
}

#[test]
fn start_boundary_formatting() {
    let date = LocalDate {
        year: 2027,
        month: 1,
        day: 5,
    };
    assert_eq!(format_start_boundary(date, 0, 0), "2027-01-05T00:00:00");
    assert_eq!(format_start_boundary(date, 23, 59), "2027-01-05T23:59:00");
    assert_eq!(format_start_boundary(today(), 7, 5), "2026-09-03T07:05:00");
    assert_eq!(parse_start_boundary("2027-01-05T23:59:00"), Some((23, 59)));
    for bad in [
        "2027-01-05T23:59:00Z",
        "2027-01-05T23:59:01",
        "2027-01-05T24:00:00",
        "2027-01-05T10:60:00",
        "2027-13-05T10:00:00",
        "2027-01-05 10:00:00",
        "2027-01-05T1a:00:00",
        "",
    ] {
        assert_eq!(parse_start_boundary(bad), None, "{bad}");
    }
}

#[test]
fn scheduled_scan_argument_parser() {
    let args = |values: &[&str]| -> Vec<OsString> { values.iter().map(OsString::from).collect() };
    assert_eq!(
        parse_scheduled_scan_args(&args(&["--scheduled-scan", ID])),
        Some(id())
    );
    let upper = ID.to_uppercase();
    let invalid: [&[&str]; 9] = [
        &[],
        &["--scheduled-scan"],
        &[ID],
        &["--scheduled-scan", "not-a-uuid"],
        &["--scheduled-scan", &upper],
        &["--SCHEDULED-SCAN", ID],
        &["--scheduled-scan", ID, "--extra"],
        &["--other", "--scheduled-scan", ID],
        &["--scheduled-scan", "{0f8fad5b-d9cb-469f-a165-70867728950e}"],
    ];
    for case in invalid {
        assert_eq!(parse_scheduled_scan_args(&args(case)), None, "{case:?}");
    }
    let generated = scheduled_scan_arguments(&id());
    let split: Vec<OsString> = generated.split(' ').map(OsString::from).collect();
    assert_eq!(parse_scheduled_scan_args(&split), Some(id()));
}

#[test]
fn access_denied_and_unavailable_are_mapped() {
    for error in [
        AdapterError::Denied,
        AdapterError::Unsupported(UnsupportedReason::ApiUnavailable),
    ] {
        let adapter = adapter(FakeTasks::failing(error));
        let change = upsert(ScheduleCadence::Daily { hour: 1, minute: 0 });
        assert_eq!(adapter.observe(&change), Err(error));
        assert_eq!(adapter.apply(&change), Err(error));
        assert_eq!(adapter.apply(&remove()), Err(error));
        assert_eq!(list_scan_schedules_with(adapter.service(), EXE), Err(error));
    }
    use windows::Win32::Foundation::{E_ACCESSDENIED, E_FAIL, REGDB_E_CLASSNOTREG};
    assert_eq!(com::map_hresult(E_ACCESSDENIED), AdapterError::Denied);
    assert_eq!(
        com::map_hresult(REGDB_E_CLASSNOTREG),
        AdapterError::Unsupported(UnsupportedReason::ApiUnavailable)
    );
    assert_eq!(com::map_hresult(E_FAIL), AdapterError::Failed);
}

#[test]
fn inverse_restores_prior_schedule() {
    let adapter = adapter(FakeTasks::default());
    let first = ScheduleCadence::Weekly {
        day: Weekday::Wednesday,
        hour: 8,
        minute: 15,
    };
    let second = ScheduleCadence::Daily {
        hour: 22,
        minute: 45,
    };

    // Upsert over none -> inverse removes.
    let prior = adapter.observe(&upsert(first)).unwrap();
    adapter.apply(&upsert(first)).unwrap();
    let inverse = upsert(first).inverse(&prior).unwrap().unwrap();
    assert_eq!(inverse, remove());
    adapter.apply(&inverse).unwrap();
    assert_eq!(
        adapter.observe(&remove()).unwrap(),
        PriorState::Schedule { cadence: None }
    );

    // Upsert over existing -> inverse restores the old cadence.
    adapter.apply(&upsert(first)).unwrap();
    let prior = adapter.observe(&upsert(second)).unwrap();
    adapter.apply(&upsert(second)).unwrap();
    let inverse = upsert(second).inverse(&prior).unwrap().unwrap();
    assert_eq!(inverse, upsert(first));
    adapter.apply(&inverse).unwrap();
    assert_eq!(
        adapter.observe(&upsert(first)).unwrap(),
        PriorState::Schedule {
            cadence: Some(first)
        }
    );

    // Remove -> inverse re-creates.
    let prior = adapter.observe(&remove()).unwrap();
    adapter.apply(&remove()).unwrap();
    assert_eq!(remove().inverse(&prior).unwrap(), Some(upsert(first)));
}

#[test]
fn describe_is_plain_and_low_risk() {
    let adapter = adapter(FakeTasks::default());
    let summary = adapter
        .describe(&upsert(ScheduleCadence::Weekly {
            day: Weekday::Sunday,
            hour: 3,
            minute: 0,
        }))
        .unwrap();
    assert!(summary.effect.contains("every Sunday at 03:00"));
    assert_eq!(summary.risk, RiskLevel::Low);
    let other = SystemChange::SetHibernation { enabled: true };
    assert!(adapter.describe(&other).is_err());
    assert_eq!(adapter.observe(&other), Err(AdapterError::Failed));
    assert_eq!(adapter.apply(&other), Err(AdapterError::Failed));
}

#[test]
fn ole_dates_convert_to_local_strings() {
    assert_eq!(com::ole_date_to_local(0.0), None);
    assert_eq!(
        com::ole_date_to_local(36526.0).as_deref(),
        Some("2000-01-01T00:00:00")
    );
    // 46268 = 2026-09-03; .125 = 03:00:00.
    assert_eq!(
        com::ole_date_to_local(46268.125).as_deref(),
        Some("2026-09-03T03:00:00")
    );
    assert_eq!(com::ole_date_to_local(f64::NAN), None);
}

fn random_schedule_id() -> ScheduleId {
    use std::{
        collections::hash_map::RandomState,
        hash::{BuildHasher, Hasher},
        time::{SystemTime, UNIX_EPOCH},
    };
    let mut words = [0_u64; 2];
    for (index, word) in words.iter_mut().enumerate() {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos()),
        );
        hasher.write_usize(index);
        hasher.write_u32(std::process::id());
        *word = hasher.finish();
    }
    let hex = format!("{:016x}{:016x}", words[0], words[1]);
    ScheduleId::parse(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
    .unwrap()
}

/// Deletes the live task even if the test panics.
struct DeleteOnDrop(String);

impl Drop for DeleteOnDrop {
    fn drop(&mut self) {
        let _ = ComTaskService.delete(&self.0);
    }
}

#[test]
fn live_disposable_task_round_trip() {
    let schedule_id = random_schedule_id();
    let name = NAMING_SCHEME.task_name(&schedule_id);
    let _guard = DeleteOnDrop(name.clone());
    let adapter = SchedulerAdapter::new();
    let cadence = ScheduleCadence::Weekly {
        day: Weekday::Sunday,
        hour: 3,
        minute: 0,
    };
    let change = SystemChange::UpsertScanSchedule {
        schedule_id: schedule_id.clone(),
        cadence,
    };
    let removal = SystemChange::RemoveScanSchedule {
        schedule_id: schedule_id.clone(),
    };

    assert_eq!(
        adapter.observe(&change).unwrap(),
        PriorState::Schedule { cadence: None }
    );
    adapter.apply(&change).unwrap();
    assert_eq!(
        adapter.observe(&change).unwrap(),
        PriorState::Schedule {
            cadence: Some(cadence)
        }
    );

    let raw = ComTaskService.get(&name).unwrap().unwrap();
    let exe = std::env::current_exe()
        .unwrap()
        .into_os_string()
        .into_string()
        .unwrap();
    assert_eq!(
        raw.actions,
        vec![RawAction::Exec {
            path: exe,
            arguments: format!("--scheduled-scan {schedule_id}"),
        }]
    );
    let listed = list_scan_schedules().unwrap();
    let mine = listed.iter().find(|item| item.id == schedule_id).unwrap();
    assert_eq!(mine.cadence, Some(cadence));
    assert!(!mine.orphaned && mine.enabled);
    assert!(mine.next_run.is_some());

    // Re-upsert updates in place.
    adapter.apply(&change).unwrap();
    assert_eq!(
        adapter.observe(&removal).unwrap(),
        PriorState::Schedule {
            cadence: Some(cadence)
        }
    );

    adapter.apply(&removal).unwrap();
    assert!(ComTaskService.get(&name).unwrap().is_none());
    assert_eq!(
        adapter.observe(&removal).unwrap(),
        PriorState::Schedule { cadence: None }
    );
    adapter.apply(&removal).unwrap();
}
