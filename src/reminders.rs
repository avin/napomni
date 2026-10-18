//! Application operations shared by the desktop and reminder tools.
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Duration, Local, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub type ReminderId = u64;
#[derive(Clone, Copy, Debug)]
pub struct TimeSample {
    pub utc: DateTime<Utc>,
    pub awake_millis: u64,
}
pub trait Clock: Send + Sync {
    fn sample(&self) -> TimeSample;
    fn local_at(&self, utc: DateTime<Utc>) -> NaiveDateTime;
    /// Zero candidates for a skipped local time, two for a repeated local time.
    fn resolve_local(&self, local: NaiveDateTime) -> Vec<DateTime<Utc>>;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn sample(&self) -> TimeSample {
        let mut ticks = 0;
        #[cfg(windows)]
        unsafe {
            let _ =
                windows::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime(&mut ticks);
        }
        TimeSample {
            utc: Utc::now(),
            awake_millis: ticks / 10_000,
        }
    }
    fn local_at(&self, utc: DateTime<Utc>) -> NaiveDateTime {
        utc.with_timezone(&Local).naive_local()
    }
    fn resolve_local(&self, local: NaiveDateTime) -> Vec<DateTime<Utc>> {
        match Local.from_local_datetime(&local) {
            chrono::LocalResult::None => vec![],
            chrono::LocalResult::Single(at) => vec![at.with_timezone(&Utc)],
            chrono::LocalResult::Ambiguous(a, b) => {
                vec![a.with_timezone(&Utc), b.with_timezone(&Utc)]
            }
        }
    }
}
#[derive(Clone, Debug)]
pub struct CommandContext {
    pub submitted_utc: DateTime<Utc>,
    pub submitted_local: NaiveDateTime,
    pub source: String,
    pub selected_id: Option<ReminderId>,
}
impl CommandContext {
    pub fn capture(clock: &dyn Clock, source: &str, selected_id: Option<ReminderId>) -> Self {
        let submitted_utc = clock.sample().utc;
        Self {
            submitted_utc,
            submitted_local: clock.local_at(submitted_utc),
            source: source.into(),
            selected_id,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Schedule {
    Once { at: DateTime<Utc> },
    OnceLocal { date_time: NaiveDateTime },
}
impl Schedule {
    pub fn next_after(&self, after: DateTime<Utc>, _clock: &dyn Clock) -> Option<DateTime<Utc>> {
        let at = match self {
            Self::Once { at } => Some(*at),
            Self::OnceLocal { date_time } => _clock.resolve_local(*date_time).into_iter().min(),
        };
        at.filter(|at| *at > after)
    }
    pub fn description(&self, clock: &dyn Clock) -> String {
        let local = match self {
            Self::Once { at } => clock.local_at(*at),
            Self::OnceLocal { date_time } => *date_time,
        };
        format!(
            "Однократно {} (местное время)",
            local.format("%d.%m.%Y в %H:%M:%S")
        )
    }
    pub fn ended_at(&self, now: DateTime<Utc>, clock: &dyn Clock) -> bool {
        match self {
            Self::Once { at } => *at <= now,
            Self::OnceLocal { date_time } => clock
                .resolve_local(*date_time)
                .into_iter()
                .min()
                .map_or(*date_time <= clock.local_at(now), |at| at <= now),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleInput {
    AfterSeconds { seconds: i64 },
    AtLocal { date_time: NaiveDateTime },
    Tomorrow { time: NaiveTime },
}
impl ScheduleInput {
    pub fn resolve(self, context: &CommandContext, clock: &dyn Clock) -> Result<Schedule> {
        let schedule = match self {
            Self::AfterSeconds { seconds } => {
                ensure!(seconds > 0, "Укажите положительное время до напоминания");
                Schedule::Once {
                    at: context
                        .submitted_utc
                        .checked_add_signed(
                            Duration::try_seconds(seconds).context("Слишком большое время")?,
                        )
                        .context("Слишком далёкое время")?,
                }
            }
            Self::AtLocal { date_time } => {
                resolve_unique(clock, date_time)?;
                Schedule::OnceLocal { date_time }
            }
            Self::Tomorrow { time } => {
                let date_time = context
                    .submitted_local
                    .date()
                    .succ_opt()
                    .context("Слишком далёкая дата")?
                    .and_time(time);
                resolve_unique(clock, date_time)?;
                Schedule::OnceLocal { date_time }
            }
        };
        ensure!(
            schedule.next_after(clock.sample().utc, clock).is_some(),
            "Выбранное время уже прошло. Уточните новое время"
        );
        Ok(schedule)
    }
}
fn resolve_unique(clock: &dyn Clock, local: NaiveDateTime) -> Result<DateTime<Utc>> {
    let candidates = clock.resolve_local(local);
    ensure!(
        candidates.len() == 1,
        "Местное время отсутствует или неоднозначно. Уточните время"
    );
    Ok(candidates[0])
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reminder {
    pub id: ReminderId,
    pub text: String,
    pub schedule: Schedule,
    pub border: u32,
    pub sound: bool,
    pub paused: bool,
    pub completed: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ToastSnapshot {
    pub reminder_id: ReminderId,
    pub text: String,
    pub border: u32,
    pub sound: bool,
    pub sequence: u64,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateReminder {
    pub text: String,
    pub schedule: ScheduleInput,
    pub border: Option<u32>,
    pub sound: Option<bool>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Persisted {
    pub reminders: Vec<Reminder>,
    pub next_id: ReminderId,
    #[serde(default)]
    pub undo: Option<crate::mutations::UndoEntry>,
}
pub trait ReminderStore: Send {
    fn load(&self) -> Result<Persisted>;
    fn save(&self, state: &Persisted) -> Result<()>;
}
pub struct JsonStore {
    path: PathBuf,
}
impl JsonStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().into(),
        }
    }
}
impl ReminderStore for JsonStore {
    fn load(&self) -> Result<Persisted> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("Не удалось прочитать напоминания"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Persisted::default()),
            Err(error) => Err(error.into()),
        }
    }
    fn save(&self, state: &Persisted) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let pending = self.path.with_extension("pending");
        let mut file = fs::File::create(&pending).context("Не удалось сохранить напоминания")?;
        use std::io::Write;
        file.write_all(&serde_json::to_vec_pretty(state)?)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&pending, &self.path).context("Не удалось заменить файл напоминаний")?;
        Ok(())
    }
}
pub struct Application {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) store: Box<dyn ReminderStore>,
    pub(crate) state: Persisted,
    pub(crate) toasts: Vec<ToastSnapshot>,
    pub(crate) last: TimeSample,
    pub(crate) sequence: u64,
    pub(crate) eligible_after: HashMap<ReminderId, DateTime<Utc>>,
}
impl Application {
    pub fn open(clock: Arc<dyn Clock>, store: Box<dyn ReminderStore>) -> Result<Self> {
        let state = store.load()?;
        let last = clock.sample();
        let mut app = Self {
            clock,
            store,
            state,
            toasts: vec![],
            last,
            sequence: 0,
            eligible_after: HashMap::new(),
        };
        app.resume()?;
        Ok(app)
    }
    pub fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }
    pub fn reminders(&self) -> &[Reminder] {
        &self.state.reminders
    }
    pub fn open_toasts(&self) -> &[ToastSnapshot] {
        &self.toasts
    }
    pub fn signal_active(&self) -> bool {
        self.toasts.iter().any(|t| t.sound)
    }
    pub fn close_toast(&mut self, id: ReminderId) {
        self.toasts.retain(|t| t.reminder_id != id);
    }
    pub fn create(&mut self, input: CreateReminder, context: &CommandContext) -> Result<Reminder> {
        let schedule = input.schedule.resolve(context, self.clock())?;
        let text = quoted_text(&context.source).unwrap_or(input.text);
        ensure!(!text.trim().is_empty(), "Укажите текст напоминания");
        let border = input.border.unwrap_or(0x5285f5);
        ensure!(border <= 0xffffff, "Цвет должен быть RGB от 0 до 16777215");
        let id = self
            .state
            .next_id
            .checked_add(1)
            .context("Закончились идентификаторы")?;
        let reminder = Reminder {
            id,
            text,
            schedule,
            border,
            sound: input.sound.unwrap_or(true),
            paused: false,
            completed: false,
        };
        let mut next = self.state.clone();
        next.next_id = id;
        next.undo = Some(crate::mutations::UndoEntry {
            id,
            before: None,
            index: next.reminders.len(),
        });
        next.reminders.push(reminder.clone());
        self.store.save(&next)?;
        self.state = next;
        self.skip_through(id, self.clock.sample().utc);
        Ok(reminder)
    }
    pub(crate) fn skip_through(&mut self, id: ReminderId, now: DateTime<Utc>) {
        self.eligible_after
            .entry(id)
            .and_modify(|at| *at = (*at).max(now))
            .or_insert(now);
    }
    /// After startup or suspension, elapsed occurrences are consumed without a toast.
    pub fn resume(&mut self) -> Result<()> {
        let now = self.clock.sample();
        let mut next = self.state.clone();
        for reminder in &mut next.reminders {
            if reminder.schedule.ended_at(now.utc, self.clock()) {
                reminder.completed = true;
            }
        }
        if next.reminders != self.state.reminders {
            self.store.save(&next)?;
            self.state = next;
        }
        self.last = now;
        Ok(())
    }
    pub fn tick(&mut self) -> Result<()> {
        let now = self.clock.sample();
        let wall_ms = (now.utc - self.last.utc).num_milliseconds();
        let awake_ms = now.awake_millis.saturating_sub(self.last.awake_millis);
        if wall_ms
            > i64::try_from(awake_ms)
                .unwrap_or(i64::MAX)
                .saturating_add(1000)
        {
            return self.resume();
        }
        let mut next = self.state.clone();
        let mut snapshots = Vec::new();
        for reminder in &mut next.reminders {
            if reminder.paused || reminder.completed {
                continue;
            }
            if reminder
                .schedule
                .next_after(self.last.utc, self.clock())
                .is_some_and(|at| at <= now.utc)
            {
                reminder.completed = reminder.schedule.ended_at(now.utc, self.clock());
                if !self.toasts.iter().any(|t| t.reminder_id == reminder.id) {
                    snapshots.push(ToastSnapshot {
                        reminder_id: reminder.id,
                        text: reminder.text.clone(),
                        border: reminder.border,
                        sound: reminder.sound,
                        sequence: 0,
                    });
                }
            } else if reminder.schedule.ended_at(now.utc, self.clock()) {
                reminder.completed = true;
            }
        }
        if next.reminders != self.state.reminders {
            self.store.save(&next)?;
            self.state = next;
        }
        for mut snapshot in snapshots {
            self.sequence += 1;
            snapshot.sequence = self.sequence;
            self.toasts.insert(0, snapshot);
        }
        self.last = now;
        Ok(())
    }
}
pub(crate) fn quoted_text(source: &str) -> Option<String> {
    for (open, close) in [('«', '»'), ('"', '"'), ('“', '”')] {
        if let Some(start) = source.find(open) {
            let rest = &source[start + open.len_utf8()..];
            if let Some(end) = rest.find(close) {
                return Some(rest[..end].to_owned());
            }
        }
    }
    None
}
