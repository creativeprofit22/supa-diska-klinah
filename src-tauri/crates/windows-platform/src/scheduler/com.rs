//! Task Scheduler 2.0 COM implementation of [`TaskService`].

use cleanup_core::system_change::UnsupportedReason;
use windows::{
    Win32::{
        Foundation::{
            E_ACCESSDENIED, E_INVALIDARG, REGDB_E_CLASSNOTREG, RPC_E_CHANGED_MODE, VARIANT_BOOL,
            VARIANT_FALSE, VARIANT_TRUE,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            TaskScheduler::{
                IDailyTrigger, IExecAction, IRegisteredTask, ITaskDefinition, ITaskFolder,
                ITaskService, IWeeklyTrigger, TASK_ACTION_EXEC, TASK_ACTION_TYPE,
                TASK_CREATE_OR_UPDATE, TASK_ENUM_HIDDEN, TASK_INSTANCES_IGNORE_NEW,
                TASK_LOGON_INTERACTIVE_TOKEN, TASK_RUNLEVEL_LUA, TASK_TRIGGER_DAILY,
                TASK_TRIGGER_TYPE2, TASK_TRIGGER_WEEKLY, TaskScheduler,
            },
            Variant::VARIANT,
        },
    },
    core::{BSTR, HRESULT, Interface},
};

use super::{
    EXECUTION_TIME_LIMIT, LocalDate, NAMING_SCHEME, RawAction, RawTask, RawTrigger,
    TASK_DESCRIPTION, TaskService, TaskSpec,
};
use crate::system_change::AdapterError;

/// Upper bound on tasks enumerated in the scheme folder.
const MAX_ENUMERATED_TASKS: i32 = 2048;
/// Upper bound on actions/triggers read per task.
const MAX_PARTS: i32 = 8;
/// Upper bound on any string read from a task definition (UTF-16 units are
/// at most this many chars).
const MAX_STRING_CHARS: usize = 2048;

const HRESULT_FILE_NOT_FOUND: HRESULT = HRESULT(0x8007_0002_u32 as i32);
const HRESULT_PATH_NOT_FOUND: HRESULT = HRESULT(0x8007_0003_u32 as i32);
const HRESULT_ALREADY_EXISTS: HRESULT = HRESULT(0x8007_00B7_u32 as i32);
const SCHED_E_SERVICE_NOT_RUNNING: HRESULT = HRESULT(0x8004_1315_u32 as i32);

/// Real Task Scheduler access for the current user.
#[derive(Clone, Copy, Debug, Default)]
pub struct ComTaskService;

fn is_not_found(code: HRESULT) -> bool {
    code == HRESULT_FILE_NOT_FOUND || code == HRESULT_PATH_NOT_FOUND
}

pub(super) fn map_hresult(code: HRESULT) -> AdapterError {
    if code == E_ACCESSDENIED {
        AdapterError::Denied
    } else if code == REGDB_E_CLASSNOTREG || code == SCHED_E_SERVICE_NOT_RUNNING {
        AdapterError::Unsupported(UnsupportedReason::ApiUnavailable)
    } else {
        AdapterError::Failed
    }
}

fn map_error(error: windows::core::Error) -> AdapterError {
    map_hresult(error.code())
}

struct ComApartment {
    uninitialize: bool,
}

impl ComApartment {
    fn enter() -> Result<Self, AdapterError> {
        // SAFETY: a null reserved pointer and COINIT_MULTITHREADED are documented arguments.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self {
                uninitialize: false,
            });
        }
        result.ok().map_err(map_error)?;
        Ok(Self { uninitialize: true })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            // SAFETY: CoInitializeEx returned S_OK/S_FALSE on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// A connected service. Field order drops the interface before COM.
struct Session {
    service: ITaskService,
    _apartment: ComApartment,
}

impl Session {
    fn open() -> Result<Self, AdapterError> {
        let apartment = ComApartment::enter()?;
        // SAFETY: COM is initialized on this thread; TaskScheduler is the documented CLSID.
        let service: ITaskService =
            unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }
                .map_err(map_error)?;
        let empty = VARIANT::default();
        // SAFETY: empty VARIANTs connect to the local machine as the current user.
        unsafe { service.Connect(&empty, &empty, &empty, &empty) }.map_err(map_error)?;
        Ok(Self {
            service,
            _apartment: apartment,
        })
    }

    fn folder(&self) -> Result<Option<ITaskFolder>, AdapterError> {
        // SAFETY: the service is connected; the path is a compiled constant.
        match unsafe { self.service.GetFolder(&BSTR::from(NAMING_SCHEME.folder)) } {
            Ok(folder) => Ok(Some(folder)),
            Err(error) if is_not_found(error.code()) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    fn folder_for_write(&self) -> Result<ITaskFolder, AdapterError> {
        if let Some(folder) = self.folder()? {
            return Ok(folder);
        }
        // Only reachable with a non-root scheme folder.
        // SAFETY: the service is connected; "\\" is the documented root path.
        let root = unsafe { self.service.GetFolder(&BSTR::from("\\")) }.map_err(map_error)?;
        // SAFETY: the folder path is a compiled constant; an empty VARIANT keeps the default SDDL.
        match unsafe { root.CreateFolder(&BSTR::from(NAMING_SCHEME.folder), &VARIANT::default()) } {
            Ok(folder) => Ok(folder),
            Err(error) if error.code() == HRESULT_ALREADY_EXISTS => {
                self.folder()?.ok_or(AdapterError::Failed)
            }
            Err(error) => Err(map_error(error)),
        }
    }
}

fn bounded(value: BSTR) -> String {
    let text = value.to_string();
    if text.chars().count() > MAX_STRING_CHARS {
        text.chars().take(MAX_STRING_CHARS).collect()
    } else {
        text
    }
}

fn read_actions(task: &ITaskDefinition) -> windows::core::Result<Vec<RawAction>> {
    // SAFETY: every call below operates on live interfaces owned by this function.
    unsafe {
        let actions = task.Actions()?;
        let mut count = 0;
        actions.Count(&mut count)?;
        if !(0..=MAX_PARTS).contains(&count) {
            return Ok(vec![RawAction::Other; 2]);
        }
        let mut result = Vec::with_capacity(count as usize);
        for index in 1..=count {
            let action = actions.get_Item(index)?;
            let mut kind = TASK_ACTION_TYPE::default();
            action.Type(&mut kind)?;
            if kind != TASK_ACTION_EXEC {
                result.push(RawAction::Other);
                continue;
            }
            let exec: IExecAction = action.cast()?;
            let (mut path, mut arguments) = (BSTR::new(), BSTR::new());
            exec.Path(&mut path)?;
            exec.Arguments(&mut arguments)?;
            result.push(RawAction::Exec {
                path: bounded(path),
                arguments: bounded(arguments),
            });
        }
        Ok(result)
    }
}

fn read_triggers(task: &ITaskDefinition) -> windows::core::Result<Vec<RawTrigger>> {
    // SAFETY: every call below operates on live interfaces owned by this function.
    unsafe {
        let triggers = task.Triggers()?;
        let mut count = 0;
        triggers.Count(&mut count)?;
        if !(0..=MAX_PARTS).contains(&count) {
            return Ok(vec![RawTrigger::Other; 2]);
        }
        let mut result = Vec::with_capacity(count as usize);
        for index in 1..=count {
            let trigger = triggers.get_Item(index)?;
            let mut kind = TASK_TRIGGER_TYPE2::default();
            trigger.Type(&mut kind)?;
            let mut start = BSTR::new();
            trigger.StartBoundary(&mut start)?;
            let start_boundary = bounded(start);
            if kind == TASK_TRIGGER_DAILY {
                let daily: IDailyTrigger = trigger.cast()?;
                let mut days_interval = 0;
                daily.DaysInterval(&mut days_interval)?;
                result.push(RawTrigger::Daily {
                    days_interval,
                    start_boundary,
                });
            } else if kind == TASK_TRIGGER_WEEKLY {
                let weekly: IWeeklyTrigger = trigger.cast()?;
                let (mut days_of_week, mut weeks_interval) = (0, 0);
                weekly.DaysOfWeek(&mut days_of_week)?;
                weekly.WeeksInterval(&mut weeks_interval)?;
                result.push(RawTrigger::Weekly {
                    days_of_week,
                    weeks_interval,
                    start_boundary,
                });
            } else {
                result.push(RawTrigger::Other);
            }
        }
        Ok(result)
    }
}

fn read_task(task: &IRegisteredTask) -> windows::core::Result<RawTask> {
    // SAFETY: the registered task interface is live for the whole call.
    unsafe {
        let definition = task.Definition()?;
        Ok(RawTask {
            name: bounded(task.Name()?),
            actions: read_actions(&definition)?,
            triggers: read_triggers(&definition)?,
            enabled: task.Enabled()?.as_bool(),
            last_run: task.LastRunTime().ok().and_then(ole_date_to_local),
            next_run: task.NextRunTime().ok().and_then(ole_date_to_local),
        })
    }
}

/// Convert an OLE automation date (local time) to `YYYY-MM-DDTHH:MM:SS`.
/// Dates before 2000 are Task Scheduler's "never" sentinels.
pub(super) fn ole_date_to_local(value: f64) -> Option<String> {
    // 36526 = 2000-01-01, 2958465 = 9999-12-31.
    if !value.is_finite() || !(36526.0..2_958_466.0).contains(&value) {
        return None;
    }
    let mut days = value.floor() as i64;
    let mut seconds = ((value - value.floor()) * 86_400.0).round() as i64;
    if seconds >= 86_400 {
        days += 1;
        seconds -= 86_400;
    }
    // Days since 1970-01-01 (OLE day 25569), then Howard Hinnant's civil_from_days.
    let z = days - 25_569 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        seconds / 3_600,
        seconds % 3_600 / 60,
        seconds % 60
    ))
}

pub(super) fn local_date() -> LocalDate {
    let mut now = windows_sys::Win32::Foundation::SYSTEMTIME::default();
    // SAFETY: now is writable storage of the documented structure.
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut now) };
    LocalDate {
        year: now.wYear,
        month: now.wMonth as u8,
        day: now.wDay as u8,
    }
}

fn variant_bool(value: bool) -> VARIANT_BOOL {
    if value { VARIANT_TRUE } else { VARIANT_FALSE }
}

fn build_definition(
    service: &ITaskService,
    spec: &TaskSpec,
) -> windows::core::Result<ITaskDefinition> {
    // SAFETY: every call below operates on live interfaces owned by this function;
    // all strings are compiled constants or derived from a validated ScheduleId
    // and the running executable path.
    unsafe {
        let definition = service.NewTask(0)?;
        definition
            .RegistrationInfo()?
            .SetDescription(&BSTR::from(TASK_DESCRIPTION))?;

        let principal = definition.Principal()?;
        principal.SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN)?;
        principal.SetRunLevel(TASK_RUNLEVEL_LUA)?;

        let settings = definition.Settings()?;
        settings.SetStartWhenAvailable(variant_bool(true))?;
        settings.SetDisallowStartIfOnBatteries(variant_bool(false))?;
        settings.SetStopIfGoingOnBatteries(variant_bool(false))?;
        settings.SetExecutionTimeLimit(&BSTR::from(EXECUTION_TIME_LIMIT))?;
        settings.SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW)?;
        settings.SetEnabled(variant_bool(true))?;

        let triggers = definition.Triggers()?;
        match &spec.trigger {
            RawTrigger::Daily {
                days_interval,
                start_boundary,
            } => {
                let trigger: IDailyTrigger = triggers.Create(TASK_TRIGGER_DAILY)?.cast()?;
                trigger.SetDaysInterval(*days_interval)?;
                trigger.SetStartBoundary(&BSTR::from(start_boundary.as_str()))?;
            }
            RawTrigger::Weekly {
                days_of_week,
                weeks_interval,
                start_boundary,
            } => {
                let trigger: IWeeklyTrigger = triggers.Create(TASK_TRIGGER_WEEKLY)?.cast()?;
                trigger.SetDaysOfWeek(*days_of_week)?;
                trigger.SetWeeksInterval(*weeks_interval)?;
                trigger.SetStartBoundary(&BSTR::from(start_boundary.as_str()))?;
            }
            RawTrigger::Other => return Err(windows::core::Error::from(E_INVALIDARG)),
        }

        let action: IExecAction = definition.Actions()?.Create(TASK_ACTION_EXEC)?.cast()?;
        action.SetPath(&BSTR::from(spec.executable.as_str()))?;
        action.SetArguments(&BSTR::from(spec.arguments.as_str()))?;
        Ok(definition)
    }
}

impl TaskService for ComTaskService {
    fn get(&self, name: &str) -> Result<Option<RawTask>, AdapterError> {
        let session = Session::open()?;
        let Some(folder) = session.folder()? else {
            return Ok(None);
        };
        // SAFETY: the folder is live; the name comes from NAMING_SCHEME.
        match unsafe { folder.GetTask(&BSTR::from(name)) } {
            Ok(task) => read_task(&task).map(Some).map_err(map_error),
            Err(error) if is_not_found(error.code()) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    fn list(&self) -> Result<Vec<RawTask>, AdapterError> {
        let session = Session::open()?;
        let Some(folder) = session.folder()? else {
            return Ok(Vec::new());
        };
        // SAFETY: the folder is live; TASK_ENUM_HIDDEN is a documented flag.
        let tasks = unsafe { folder.GetTasks(TASK_ENUM_HIDDEN.0) }.map_err(map_error)?;
        // SAFETY: the collection is live.
        let count = unsafe { tasks.Count() }.map_err(map_error)?;
        let mut result = Vec::new();
        for index in 1..=count.clamp(0, MAX_ENUMERATED_TASKS) {
            // SAFETY: index is within 1..=Count of the live collection.
            let Ok(task) = (unsafe { tasks.get_Item(&VARIANT::from(index)) }) else {
                continue;
            };
            // SAFETY: the registered task interface is live.
            let Ok(name) = (unsafe { task.Name() }) else {
                continue;
            };
            if NAMING_SCHEME.parse_task_name(&bounded(name)).is_none() {
                continue;
            }
            // A task we cannot read (for example another user's) is skipped.
            if let Ok(raw) = read_task(&task) {
                result.push(raw);
            }
        }
        Ok(result)
    }

    fn register(&self, name: &str, spec: &TaskSpec) -> Result<(), AdapterError> {
        let session = Session::open()?;
        let definition = build_definition(&session.service, spec).map_err(map_error)?;
        let folder = session.folder_for_write()?;
        let empty = VARIANT::default();
        // SAFETY: the folder and definition are live; empty VARIANTs select the
        // current user with an interactive token and the default SDDL.
        unsafe {
            folder.RegisterTaskDefinition(
                &BSTR::from(name),
                &definition,
                TASK_CREATE_OR_UPDATE.0,
                &empty,
                &empty,
                TASK_LOGON_INTERACTIVE_TOKEN,
                &empty,
            )
        }
        .map(|_| ())
        .map_err(map_error)
    }

    fn delete(&self, name: &str) -> Result<(), AdapterError> {
        let session = Session::open()?;
        let Some(folder) = session.folder()? else {
            return Ok(());
        };
        // SAFETY: the folder is live; the name comes from NAMING_SCHEME.
        match unsafe { folder.DeleteTask(&BSTR::from(name), 0) } {
            Ok(()) => Ok(()),
            Err(error) if is_not_found(error.code()) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}
