//! The application's own scheduled read-only scans.
//!
//! Tasks are registered through the Task Scheduler 2.0 COM API only (see
//! [`com`]); no command line or XML is ever built from input. Every task name
//! and argument string is derived from a validated lowercase [`ScheduleId`].
//! Tasks that carry our naming scheme but do not look exactly like something
//! this module registered are never overwritten.

mod com;
mod scan_run;

#[cfg(test)]
mod tests;

use std::ffi::OsString;

use cleanup_core::system_change::{
    ImpactSummary, PriorState, RestartRequirement, RiskLevel, ScheduleCadence, ScheduleId,
    SystemChange, UnsupportedReason, Weekday,
};
use serde::Serialize;

use crate::system_change::{AdapterError, SystemAdapter};

pub use com::ComTaskService;
pub use scan_run::{ScheduledScanSummary, read_summaries, run_scheduled_scan};

/// Where our tasks live and how they are named.
///
/// Verified empirically from a Medium-integrity (unelevated) token:
/// `ITaskFolder::CreateFolder("\\SupaDiskaKlinah")` on the root folder
/// succeeds for a standard user, so tasks live in a dedicated folder. The
/// folder is created on first registration and left in place afterwards.
/// (Fallback, if that ever changes: root folder with the prefix
/// `SupaDiskaKlinah-scan-`.)
pub const NAMING_SCHEME: NamingScheme = NamingScheme {
    folder: "\\SupaDiskaKlinah",
    task_prefix: "scan-",
};

/// Command-line switch the scheduled task passes to the application.
pub const SCHEDULED_SCAN_SWITCH: &str = "--scheduled-scan";
pub const TASK_DESCRIPTION: &str = "Supa Diska Klinah read-only scheduled scan";
pub const EXECUTION_TIME_LIMIT: &str = "PT1H";

/// Upper bound on schedules returned by the inventory.
pub const MAX_SCHEDULES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NamingScheme {
    /// Task Scheduler folder path.
    pub folder: &'static str,
    /// Task name prefix; the full name is `<prefix><uuid>`.
    pub task_prefix: &'static str,
}

impl NamingScheme {
    pub fn task_name(&self, id: &ScheduleId) -> String {
        format!("{}{}", self.task_prefix, id.as_str())
    }

    /// Returns the schedule identifier when `name` follows this scheme.
    pub fn parse_task_name(&self, name: &str) -> Option<ScheduleId> {
        name.strip_prefix(self.task_prefix)
            .and_then(|rest| ScheduleId::parse(rest).ok())
    }
}

/// Parse the application's arguments (argv[0] already stripped). Accepts
/// exactly `["--scheduled-scan", <lowercase uuid>]`.
pub fn parse_scheduled_scan_args(args: &[OsString]) -> Option<ScheduleId> {
    match args {
        [switch, id] if switch.to_str() == Some(SCHEDULED_SCAN_SWITCH) => {
            ScheduleId::parse(id.to_str()?).ok()
        }
        _ => None,
    }
}

pub fn scheduled_scan_arguments(id: &ScheduleId) -> String {
    format!("{SCHEDULED_SCAN_SWITCH} {}", id.as_str())
}

/// A local calendar date.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

/// `YYYY-MM-DDTHH:MM:00` in local time (no zone suffix).
pub fn format_start_boundary(date: LocalDate, hour: u8, minute: u8) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:00",
        date.year, date.month, date.day, hour, minute
    )
}

/// Extract `(hour, minute)` from a `YYYY-MM-DDTHH:MM:00` boundary. Anything
/// else (seconds, zone suffixes, other shapes) is rejected.
fn parse_start_boundary(value: &str) -> Option<(u8, u8)> {
    let bytes = value.as_bytes();
    if bytes.len() != 19 {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let slice = bytes.get(range)?;
        if slice.iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(slice).ok()?.parse().ok()
        } else {
            None
        }
    };
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if separators.iter().any(|&(index, byte)| bytes[index] != byte) {
        return None;
    }
    let (_year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || second != 0 {
        return None;
    }
    if hour < 24 && minute < 60 {
        Some((hour as u8, minute as u8))
    } else {
        None
    }
}

pub fn weekday_bit(day: Weekday) -> i16 {
    match day {
        Weekday::Sunday => 0x01,
        Weekday::Monday => 0x02,
        Weekday::Tuesday => 0x04,
        Weekday::Wednesday => 0x08,
        Weekday::Thursday => 0x10,
        Weekday::Friday => 0x20,
        Weekday::Saturday => 0x40,
    }
}

fn weekday_from_bit(bits: i16) -> Option<Weekday> {
    [
        Weekday::Sunday,
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
    ]
    .into_iter()
    .find(|&day| weekday_bit(day) == bits)
}

/// An action as read from (or written to) Task Scheduler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RawAction {
    Exec { path: String, arguments: String },
    Other,
}

/// A trigger as read from (or written to) Task Scheduler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RawTrigger {
    Daily {
        days_interval: i16,
        start_boundary: String,
    },
    Weekly {
        days_of_week: i16,
        weeks_interval: i16,
        start_boundary: String,
    },
    Other,
}

/// A registered task's relevant parts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawTask {
    pub name: String,
    pub actions: Vec<RawAction>,
    pub triggers: Vec<RawTrigger>,
    pub enabled: bool,
    /// Local `YYYY-MM-DDTHH:MM:SS`, `None` if the task never ran.
    pub last_run: Option<String>,
    /// Local `YYYY-MM-DDTHH:MM:SS`, `None` if nothing is scheduled.
    pub next_run: Option<String>,
}

/// The single definition shape this module registers. The COM layer adds the
/// fixed principal, settings, and description.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskSpec {
    pub executable: String,
    pub arguments: String,
    pub trigger: RawTrigger,
}

impl TaskSpec {
    pub fn new(
        id: &ScheduleId,
        cadence: ScheduleCadence,
        executable: &str,
        today: LocalDate,
    ) -> Self {
        let trigger = match cadence {
            ScheduleCadence::Daily { hour, minute } => RawTrigger::Daily {
                days_interval: 1,
                start_boundary: format_start_boundary(today, hour, minute),
            },
            ScheduleCadence::Weekly { day, hour, minute } => RawTrigger::Weekly {
                days_of_week: weekday_bit(day),
                weeks_interval: 1,
                start_boundary: format_start_boundary(today, hour, minute),
            },
        };
        Self {
            executable: executable.to_owned(),
            arguments: scheduled_scan_arguments(id),
            trigger,
        }
    }
}

/// Task Scheduler access. Names are always produced by [`NAMING_SCHEME`].
pub trait TaskService: Send + Sync {
    /// `Ok(None)` when the task does not exist.
    fn get(&self, name: &str) -> Result<Option<RawTask>, AdapterError>;
    /// Tasks in the scheme folder whose name carries the scheme prefix.
    fn list(&self) -> Result<Vec<RawTask>, AdapterError>;
    /// Create or update (`TASK_CREATE_OR_UPDATE`).
    fn register(&self, name: &str, spec: &TaskSpec) -> Result<(), AdapterError>;
    /// Delete; an already-missing task is success.
    fn delete(&self, name: &str) -> Result<(), AdapterError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OrphanReason {
    /// The action runs a different executable than this installation.
    ForeignExecutable,
    /// The arguments are not exactly `--scheduled-scan <this task's uuid>`.
    MalformedArguments,
    /// Actions or triggers do not match anything this module registers; the
    /// task is listed but never modified.
    UnrecognizedDefinition,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSchedule {
    pub id: ScheduleId,
    pub cadence: Option<ScheduleCadence>,
    pub enabled: bool,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
    pub orphaned: bool,
    pub orphan_reason: Option<OrphanReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Classified {
    Current(ScheduleCadence),
    Orphaned(ScheduleCadence, OrphanReason),
    Unrecognized,
}

fn decode_trigger(trigger: &RawTrigger) -> Option<ScheduleCadence> {
    match trigger {
        RawTrigger::Daily {
            days_interval: 1,
            start_boundary,
        } => {
            let (hour, minute) = parse_start_boundary(start_boundary)?;
            Some(ScheduleCadence::Daily { hour, minute })
        }
        RawTrigger::Weekly {
            days_of_week,
            weeks_interval: 1,
            start_boundary,
        } => {
            let day = weekday_from_bit(*days_of_week)?;
            let (hour, minute) = parse_start_boundary(start_boundary)?;
            Some(ScheduleCadence::Weekly { day, hour, minute })
        }
        _ => None,
    }
}

fn same_path(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .chars()
            .flat_map(char::to_lowercase)
            .eq(right.chars().flat_map(char::to_lowercase))
}

fn classify(task: &RawTask, id: &ScheduleId, executable: &str) -> Classified {
    let (path, arguments) = match task.actions.as_slice() {
        [RawAction::Exec { path, arguments }] => (path, arguments),
        _ => return Classified::Unrecognized,
    };
    let cadence = match task.triggers.as_slice() {
        [trigger] => match decode_trigger(trigger) {
            Some(cadence) => cadence,
            None => return Classified::Unrecognized,
        },
        _ => return Classified::Unrecognized,
    };
    if *arguments != scheduled_scan_arguments(id) {
        Classified::Orphaned(cadence, OrphanReason::MalformedArguments)
    } else if !same_path(path, executable) {
        Classified::Orphaned(cadence, OrphanReason::ForeignExecutable)
    } else {
        Classified::Current(cadence)
    }
}

fn to_schedule(task: RawTask, executable: &str) -> Option<ScanSchedule> {
    let id = NAMING_SCHEME.parse_task_name(&task.name)?;
    let (cadence, orphan_reason) = match classify(&task, &id, executable) {
        Classified::Current(cadence) => (Some(cadence), None),
        Classified::Orphaned(cadence, reason) => (Some(cadence), Some(reason)),
        Classified::Unrecognized => (None, Some(OrphanReason::UnrecognizedDefinition)),
    };
    Some(ScanSchedule {
        id,
        cadence,
        enabled: task.enabled,
        last_run: task.last_run,
        next_run: task.next_run,
        orphaned: orphan_reason.is_some(),
        orphan_reason,
    })
}

fn current_executable() -> Option<String> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.into_os_string().into_string().ok())
}

/// Inventory of this application's scheduled scans, using the real Task
/// Scheduler and the running executable.
pub fn list_scan_schedules() -> Result<Vec<ScanSchedule>, AdapterError> {
    let executable = current_executable().ok_or(AdapterError::Failed)?;
    list_scan_schedules_with(&ComTaskService, &executable)
}

pub fn list_scan_schedules_with(
    service: &dyn TaskService,
    executable: &str,
) -> Result<Vec<ScanSchedule>, AdapterError> {
    let mut schedules: Vec<_> = service
        .list()?
        .into_iter()
        .filter_map(|task| to_schedule(task, executable))
        .take(MAX_SCHEDULES)
        .collect();
    schedules.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(schedules)
}

pub struct SchedulerAdapter<S: TaskService = ComTaskService> {
    service: S,
    executable: Option<String>,
    today: fn() -> LocalDate,
}

impl SchedulerAdapter<ComTaskService> {
    pub fn new() -> Self {
        Self {
            service: ComTaskService,
            executable: current_executable(),
            today: com::local_date,
        }
    }
}

impl Default for SchedulerAdapter<ComTaskService> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: TaskService> SchedulerAdapter<S> {
    pub fn with_service(service: S, executable: &str, today: fn() -> LocalDate) -> Self {
        Self {
            service,
            executable: Some(executable.to_owned()),
            today,
        }
    }

    pub fn service(&self) -> &S {
        &self.service
    }

    fn executable(&self) -> Result<&str, AdapterError> {
        self.executable.as_deref().ok_or(AdapterError::Failed)
    }

    /// Current cadence of our task. `allow_orphan` accepts tasks whose action
    /// no longer points at this executable (removal only).
    fn current(
        &self,
        id: &ScheduleId,
        allow_orphan: bool,
    ) -> Result<Option<ScheduleCadence>, AdapterError> {
        let executable = self.executable()?;
        let Some(task) = self.service.get(&NAMING_SCHEME.task_name(id))? else {
            return Ok(None);
        };
        match classify(&task, id, executable) {
            Classified::Current(cadence) => Ok(Some(cadence)),
            Classified::Orphaned(cadence, _) if allow_orphan => Ok(Some(cadence)),
            _ => Err(AdapterError::Unsupported(UnsupportedReason::NotPresent)),
        }
    }
}

fn describe_cadence(cadence: &ScheduleCadence) -> String {
    match *cadence {
        ScheduleCadence::Daily { hour, minute } => format!("every day at {hour:02}:{minute:02}"),
        ScheduleCadence::Weekly { day, hour, minute } => {
            format!("every {day:?} at {hour:02}:{minute:02}")
        }
    }
}

impl<S: TaskService> SystemAdapter for SchedulerAdapter<S> {
    fn describe(&self, change: &SystemChange) -> Result<ImpactSummary, AdapterError> {
        let effect = match change {
            SystemChange::UpsertScanSchedule { cadence, .. } => {
                cadence
                    .validate()
                    .map_err(|_| AdapterError::Unsupported(UnsupportedReason::NotPresent))?;
                format!(
                    "Run a read-only disk scan {} as your user, without administrator rights",
                    describe_cadence(cadence)
                )
            }
            SystemChange::RemoveScanSchedule { .. } => {
                "Remove this scheduled read-only disk scan".to_owned()
            }
            _ => return Err(AdapterError::Failed),
        };
        Ok(ImpactSummary {
            component: "Scheduled scan (Task Scheduler)".to_owned(),
            effect,
            restart: RestartRequirement::None,
            risk: RiskLevel::Low,
        })
    }

    fn observe(&self, change: &SystemChange) -> Result<PriorState, AdapterError> {
        let cadence = match change {
            SystemChange::UpsertScanSchedule { schedule_id, .. } => {
                self.current(schedule_id, false)?
            }
            SystemChange::RemoveScanSchedule { schedule_id } => self.current(schedule_id, true)?,
            _ => return Err(AdapterError::Failed),
        };
        Ok(PriorState::Schedule { cadence })
    }

    fn apply(&self, change: &SystemChange) -> Result<(), AdapterError> {
        match change {
            SystemChange::UpsertScanSchedule {
                schedule_id,
                cadence,
            } => {
                cadence
                    .validate()
                    .map_err(|_| AdapterError::Unsupported(UnsupportedReason::NotPresent))?;
                // Re-check right before writing: never overwrite a foreign task.
                self.current(schedule_id, false)?;
                let spec = TaskSpec::new(schedule_id, *cadence, self.executable()?, (self.today)());
                self.service
                    .register(&NAMING_SCHEME.task_name(schedule_id), &spec)
            }
            SystemChange::RemoveScanSchedule { schedule_id } => {
                if self.current(schedule_id, true)?.is_none() {
                    return Ok(());
                }
                self.service.delete(&NAMING_SCHEME.task_name(schedule_id))
            }
            _ => Err(AdapterError::Failed),
        }
    }
}
