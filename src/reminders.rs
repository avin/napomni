//! Application operations shared by the desktop and reminder tools.
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub type ReminderId = u64;
/// Identity of a card, independent of any saved reminder.
pub type ToastId = uuid::Uuid;
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandContext {
    pub id: uuid::Uuid,
    pub submitted_utc: DateTime<Utc>,
    pub submitted_local: NaiveDateTime,
    pub source: String,
    pub selected_id: Option<ReminderId>,
}
impl CommandContext {
    pub fn capture(clock: &dyn Clock, source: &str, selected_id: Option<ReminderId>) -> Self {
        let submitted_utc = clock.sample().utc;
        Self {
            id: uuid::Uuid::new_v4(),
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
    Constrained {
        base: Box<Schedule>,
        limits: crate::constraints::Constraints,
    },
    Yearly {
        rule: crate::calendar_dates::Yearly,
    },
    Monthly {
        rule: crate::calendar_dates::Monthly,
    },
    Once {
        at: DateTime<Utc>,
    },
    IntervalAfterClose {
        next_utc: Option<DateTime<Utc>>,
        period_seconds: i64,
    },
    Interval {
        anchor_utc: DateTime<Utc>,
        period_seconds: i64,
    },
    OnceLocal {
        date_time: NaiveDateTime,
    },
    Weekly {
        rule: crate::weekly::Weekly,
    },
}
impl Schedule {
    pub fn next_after(&self, after: DateTime<Utc>, clock: &dyn Clock) -> Option<DateTime<Utc>> {
        self.next_after_cursor(after, clock, None)
    }
    pub fn next_after_cursor(
        &self,
        after: DateTime<Utc>,
        clock: &dyn Clock,
        cursor: Option<NaiveDateTime>,
    ) -> Option<DateTime<Utc>> {
        let at = match self {
            Self::Once { at } => Some(*at),
            Self::OnceLocal { date_time } => clock.resolve_local(*date_time).into_iter().min(),
            Self::Interval {
                anchor_utc,
                period_seconds,
            } => crate::interval::next_after(*anchor_utc, *period_seconds, after),
            Self::IntervalAfterClose {
                next_utc,
                period_seconds,
            } => crate::interval::next_after((*next_utc)?, *period_seconds, after),
            Self::Constrained { base, limits } => limits.next_after(base, after, clock, cursor),
            Self::Weekly { rule } => rule.next_after(after, clock, cursor),
            Self::Monthly { rule } => rule.next_after(after, clock, cursor),
            Self::Yearly { rule } => rule.next_after(after, clock, cursor),
        };
        at.filter(|at| *at > after)
    }
    /// Compare recurrence settings without the runtime deadline advanced by closing a toast.
    pub(crate) fn same_rule(&self, other: &Self) -> bool {
        let mut left = self.clone();
        let mut right = other.clone();
        if let Some((next, _)) = left.after_close_next_mut() {
            *next = None;
        }
        if let Some((next, _)) = right.after_close_next_mut() {
            *next = None;
        }
        left == right
    }
    pub fn is_after_close(&self) -> bool {
        match self {
            Self::Constrained { base, .. } => base.is_after_close(),
            Self::IntervalAfterClose { .. } => true,
            _ => false,
        }
    }
    pub(crate) fn after_close_next_mut(&mut self) -> Option<(&mut Option<DateTime<Utc>>, i64)> {
        match self {
            Self::Constrained { base, .. } => base.after_close_next_mut(),
            Self::IntervalAfterClose {
                next_utc,
                period_seconds,
            } => Some((next_utc, *period_seconds)),
            _ => None,
        }
    }
    fn after_close_anchor(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Constrained { base, .. } => base.after_close_anchor(),
            Self::IntervalAfterClose { next_utc, .. } => *next_utc,
            _ => None,
        }
    }
    fn after_close_needs_restart(&self, through: DateTime<Utc>, clock: &dyn Clock) -> bool {
        let Some(anchor) = self.after_close_anchor() else {
            return true;
        };
        anchor
            .checked_sub_signed(Duration::nanoseconds(1))
            .and_then(|before| self.next_after(before, clock))
            .is_some_and(|deadline| deadline <= through)
    }
    pub(crate) fn restart_after_close(&mut self, now: DateTime<Utc>) -> Result<()> {
        if let Some((next, seconds)) = self.after_close_next_mut() {
            *next = Some(
                now.checked_add_signed(
                    Duration::try_seconds(seconds).context("Слишком большой интервал")?,
                )
                .context("Слишком далёкое время")?,
            );
        }
        Ok(())
    }
    pub fn is_calendar_repeating(&self) -> bool {
        if let Self::Constrained { base, .. } = self {
            return base.is_calendar_repeating();
        }
        matches!(
            self,
            Self::Weekly { .. } | Self::Monthly { .. } | Self::Yearly { .. }
        )
    }
    pub fn description(&self, clock: &dyn Clock) -> String {
        let local = match self {
            Self::Constrained { base, limits } => {
                return format!("{}. {}", base.description(clock), limits.description());
            }
            Self::Once { at } => clock.local_at(*at),
            Self::Interval { period_seconds, .. } => {
                return crate::interval::description(*period_seconds);
            }
            Self::IntervalAfterClose { period_seconds, .. } => {
                return format!(
                    "Через {} после закрытия",
                    crate::interval::duration_description(*period_seconds)
                );
            }
            Self::OnceLocal { date_time } => *date_time,
            Self::Weekly { rule } => return rule.description(),
            Self::Monthly { rule } => return rule.description(),
            Self::Yearly { rule } => return rule.description(),
        };
        format!(
            "Однократно {} (местное время)",
            local.format("%d.%m.%Y в %H:%M:%S")
        )
    }
    pub fn ended_at(&self, now: DateTime<Utc>, clock: &dyn Clock) -> bool {
        match self {
            Self::Constrained { limits, .. } => limits.ended_at(now, clock),
            Self::Once { at } => *at <= now,
            Self::Weekly { .. } => false,
            Self::Monthly { .. } => false,
            Self::Yearly { .. } => false,
            Self::Interval { .. } | Self::IntervalAfterClose { .. } => false,
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
    Constrained {
        base: Box<ScheduleInput>,
        limits: crate::constraints::Constraints,
    },
    Yearly {
        dates: Vec<crate::calendar_dates::YearlyDate>,
        times: Vec<NaiveTime>,
    },
    Monthly {
        days: Vec<u32>,
        times: Vec<NaiveTime>,
    },
    AfterSeconds {
        seconds: i64,
    },
    IntervalAfterClose {
        seconds: i64,
        #[serde(default)]
        start_local: Option<NaiveDateTime>,
    },
    Interval {
        seconds: i64,
        #[serde(default)]
        start_local: Option<NaiveDateTime>,
    },
    AtLocal {
        date_time: NaiveDateTime,
    },
    Tomorrow {
        time: NaiveTime,
    },
    Weekly {
        weekdays: Vec<u32>,
        times: Vec<NaiveTime>,
        every_n_weeks: u32,
        anchor_monday: Option<NaiveDate>,
    },
}
impl ScheduleInput {
    pub fn resolve(self, context: &CommandContext, clock: &dyn Clock) -> Result<Schedule> {
        let schedule = match self {
            Self::Constrained { base, limits } => {
                ensure!(
                    !matches!(*base, Self::Constrained { .. }),
                    "Вложенные ограничения не поддерживаются"
                );
                let base = base.resolve(context, clock)?;
                limits.validate(&base)?;
                Schedule::Constrained {
                    base: Box::new(base),
                    limits,
                }
            }
            Self::Yearly { dates, times } => Schedule::Yearly {
                rule: crate::calendar_dates::Yearly::new(dates, times)?,
            },
            Self::Monthly { days, times } => Schedule::Monthly {
                rule: crate::calendar_dates::Monthly::new(days, times)?,
            },
            Self::Weekly {
                weekdays,
                times,
                every_n_weeks,
                anchor_monday,
            } => Schedule::Weekly {
                rule: crate::weekly::Weekly::new(
                    weekdays,
                    times,
                    every_n_weeks,
                    anchor_monday,
                    context.submitted_local.date(),
                )?,
            },
            Self::Interval {
                seconds,
                start_local,
            } => crate::interval::resolve(seconds, start_local, clock)?,
            Self::IntervalAfterClose {
                seconds,
                start_local,
            } => {
                let Schedule::Interval {
                    anchor_utc,
                    period_seconds,
                } = crate::interval::resolve(seconds, start_local, clock)?
                else {
                    unreachable!()
                };
                Schedule::IntervalAfterClose {
                    next_utc: crate::interval::next_after(
                        anchor_utc,
                        period_seconds,
                        clock.sample().utc,
                    ),
                    period_seconds,
                }
            }
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
            if matches!(schedule, Schedule::Constrained { .. }) {
                "Сочетание ограничений не даёт будущего срабатывания в поддерживаемом поиске: до 400 календарных лет и 100000 кандидатов. Уточните правило"
            } else {
                "Выбранное время уже прошло. Уточните новое время"
            }
        );
        Ok(schedule)
    }
}
pub(crate) fn resolve_unique(clock: &dyn Clock, local: NaiveDateTime) -> Result<DateTime<Utc>> {
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
    /// Last consumed local time; prevents replay after a wall-clock rollback.
    #[serde(default)]
    pub calendar_cursor: Option<NaiveDateTime>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToastSnapshot {
    #[serde(default = "uuid::Uuid::new_v4")]
    pub id: ToastId,
    pub reminder_id: Option<ReminderId>,
    pub text: String,
    pub border: u32,
    pub sound: bool,
    pub sequence: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnoozedToast {
    pub snapshot: ToastSnapshot,
    pub snoozed_at: DateTime<Utc>,
    pub at: DateTime<Utc>,
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
    pub snoozed: Vec<SnoozedToast>,
    #[serde(default)]
    pub undo: Option<crate::mutations::UndoEntry>,
    #[serde(default)]
    pub tool_receipts: HashMap<String, crate::commands::ToolReceipt>,
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
struct StagedStore;
impl ReminderStore for StagedStore {
    fn load(&self) -> Result<Persisted> {
        Ok(Persisted::default())
    }
    fn save(&self, _: &Persisted) -> Result<()> {
        Ok(())
    }
}
impl Application {
    /// Apply mutations and their durable receipt in a single real storage write.
    /// Failed operations leave both saved data and runtime scheduling state intact.
    pub(crate) fn transaction<R>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<R>,
    ) -> Result<R> {
        let mut staged = Self {
            clock: self.clock.clone(),
            store: Box::new(StagedStore),
            state: self.state.clone(),
            toasts: self.toasts.clone(),
            last: self.last,
            sequence: self.sequence,
            eligible_after: self.eligible_after.clone(),
        };
        let result = operation(&mut staged)?;
        self.store.save(&staged.state)?;
        std::mem::swap(&mut self.store, &mut staged.store);
        *self = staged;
        Ok(result)
    }
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
        app.resume_scheduling(true)?;
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
    pub(crate) fn skip_through(&mut self, id: ReminderId, now: DateTime<Utc>) {
        let after = self.eligible_after.entry(id).or_insert(now);
        *after = (*after).max(now);
    }
    /// Hide this occurrence without closing it or changing the recurrence phase.
    pub fn snooze_toast(&mut self, id: ToastId, minutes: u32) -> Result<()> {
        ensure!(minutes > 0, "Укажите положительное число минут");
        let Some(snapshot) = self.toasts.iter().find(|t| t.id == id).cloned() else {
            return Ok(());
        };
        let reminder_id = snapshot.reminder_id.context("Toast has no reminder")?;
        let reminder = self
            .state
            .reminders
            .iter()
            .find(|r| r.id == reminder_id)
            .context("Напоминание удалено")?;
        ensure!(!reminder.paused, "Напоминание на паузе");
        let now = self.clock.sample().utc;
        let at = now
            .checked_add_signed(Duration::minutes(i64::from(minutes)))
            .context("Слишком далёкое время")?;
        let mut next = self.state.clone();
        next.snoozed.retain(|s| s.snapshot.id != id);
        next.snoozed.push(SnoozedToast {
            snapshot,
            snoozed_at: now,
            at,
        });
        self.store.save(&next)?;
        self.state = next;
        self.toasts.retain(|t| t.id != id);
        Ok(())
    }
    pub fn close_toast(&mut self, id: ToastId) -> Result<()> {
        if let Some(snapshot) = self.toasts.iter().find(|t| t.id == id) {
            let reminder_id = snapshot.reminder_id;
            let now = self.clock.sample().utc;
            let mut next = self.state.clone();
            if let Some(reminder) = next
                .reminders
                .iter_mut()
                .find(|r| Some(r.id) == reminder_id)
            {
                reminder.schedule.restart_after_close(now)?;
            }
            if next.reminders != self.state.reminders {
                self.store.save(&next)?;
                self.state = next;
            }
            if let Some(reminder_id) = reminder_id {
                self.skip_through(reminder_id, now);
            }
            self.toasts.retain(|t| t.id != id);
        }
        Ok(())
    }
    pub fn schedule_status(&self, id: ReminderId) -> Option<String> {
        let reminder = self.state.reminders.iter().find(|r| r.id == id)?;
        if reminder.paused {
            return None;
        }
        let now = self.clock.sample().utc;
        let cursor = self
            .eligible_after
            .get(&id)
            .copied()
            .unwrap_or(self.last.utc)
            .max(self.last.utc);
        let scheduled = (!reminder.completed)
            .then(|| {
                reminder
                    .schedule
                    .next_after_cursor(cursor, self.clock(), reminder.calendar_cursor)
            })
            .flatten();
        if let Some(snoozed) = self
            .state
            .snoozed
            .iter()
            .find(|s| s.snapshot.reminder_id == Some(id))
        {
            let at = scheduled
                .filter(|at| *at <= snoozed.at)
                .unwrap_or(snoozed.at);
            if at <= now {
                return Some("Ожидает показа уведомления".into());
            }
            let label = if scheduled.is_some_and(|regular| regular <= snoozed.at) {
                "Следующее"
            } else {
                "Отложено до"
            };
            return Some(format!(
                "{label} {}",
                self.clock.local_at(at).format("%d.%m.%Y в %H:%M:%S")
            ));
        }
        if reminder.completed {
            return None;
        }
        if reminder.schedule.is_after_close()
            && self.toasts.iter().any(|t| t.reminder_id == Some(id))
        {
            return Some("Ожидает закрытия уведомления".into());
        }
        // A due occurrence is pending work, never a prediction of a later grid point.
        if scheduled.is_some_and(|at| at <= now)
            && !self.toasts.iter().any(|t| t.reminder_id == Some(id))
        {
            return Some("Ожидает показа уведомления".into());
        }
        reminder
            .schedule
            .next_after_cursor(now.max(cursor), self.clock(), reminder.calendar_cursor)
            .map(|at| {
                format!(
                    "Следующее {}",
                    self.clock.local_at(at).format("%d.%m.%Y в %H:%M:%S")
                )
            })
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
        let calendar_cursor = schedule
            .is_calendar_repeating()
            .then(|| self.clock.local_at(self.clock.sample().utc));
        let reminder = Reminder {
            id,
            text,
            schedule,
            border,
            sound: input.sound.unwrap_or(true),
            paused: false,
            completed: false,
            calendar_cursor,
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
    fn take_due_snoozes(
        &self,
        next: &mut Persisted,
        now: DateTime<Utc>,
        scheduled: &[ReminderId],
    ) -> Vec<ToastSnapshot> {
        let mut snapshots = Vec::new();
        next.snoozed.retain(|snoozed| {
            let Some(id) = snoozed.snapshot.reminder_id else {
                return false;
            };
            let Some(reminder) = next.reminders.iter().find(|r| r.id == id) else {
                return false;
            };
            if reminder.paused || scheduled.contains(&id) {
                return false;
            }
            // Also detect regular occurrences skipped during sleep or downtime.
            if !reminder.schedule.is_after_close()
                && reminder
                    .schedule
                    .next_after(snoozed.snoozed_at, self.clock())
                    .is_some_and(|at| at <= snoozed.at && at <= now)
            {
                return false;
            }
            if snoozed.at <= now {
                if !self.toasts.iter().any(|t| t.reminder_id == Some(id)) {
                    snapshots.push(snoozed.snapshot.clone());
                }
                return false;
            }
            true
        });
        snapshots
    }
    fn show_snapshots(&mut self, snapshots: Vec<ToastSnapshot>) {
        for mut snapshot in snapshots {
            self.sequence += 1;
            snapshot.sequence = self.sequence;
            self.toasts.insert(0, snapshot);
        }
    }
    /// After startup or suspension, elapsed scheduled occurrences are consumed without a toast.
    /// Explicitly snoozed occurrences remain eligible for replay.
    pub fn resume(&mut self) -> Result<()> {
        self.resume_scheduling(false)
    }
    fn resume_scheduling(&mut self, restart_after_close: bool) -> Result<()> {
        let now = self.clock.sample();
        let mut next = self.state.clone();
        if restart_after_close {
            next.snoozed.retain(|s| {
                !next
                    .reminders
                    .iter()
                    .any(|r| Some(r.id) == s.snapshot.reminder_id && r.schedule.is_after_close())
            });
        }
        for reminder in &mut next.reminders {
            let missing_occurrence = !self
                .toasts
                .iter()
                .any(|t| t.reminder_id == Some(reminder.id))
                && !self
                    .state
                    .snoozed
                    .iter()
                    .any(|s| s.snapshot.reminder_id == Some(reminder.id))
                && reminder
                    .schedule
                    .after_close_needs_restart(now.utc, self.clock());
            if reminder.schedule.is_after_close() && (restart_after_close || missing_occurrence) {
                reminder.schedule.restart_after_close(now.utc)?;
            }
            if reminder.schedule.is_calendar_repeating() {
                let local_now = self.clock.local_at(now.utc);
                reminder.calendar_cursor = Some(
                    reminder
                        .calendar_cursor
                        .map_or(local_now, |cursor| cursor.max(local_now)),
                );
            }
            if reminder.schedule.ended_at(now.utc, self.clock()) {
                reminder.completed = true;
            }
        }
        let snapshots = self.take_due_snoozes(&mut next, now.utc, &[]);
        if next.reminders != self.state.reminders || next.snoozed != self.state.snoozed {
            self.store.save(&next)?;
            self.state = next;
        }
        self.show_snapshots(snapshots);
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
        let mut consumed = Vec::new();
        for reminder in &mut next.reminders {
            if reminder.paused || reminder.completed {
                continue;
            }
            if reminder.schedule.is_after_close() {
                if self
                    .toasts
                    .iter()
                    .any(|t| t.reminder_id == Some(reminder.id))
                    || self
                        .state
                        .snoozed
                        .iter()
                        .any(|s| s.snapshot.reminder_id == Some(reminder.id))
                {
                    continue;
                }
                let consumed_through = self
                    .eligible_after
                    .get(&reminder.id)
                    .copied()
                    .unwrap_or(self.last.utc)
                    .max(self.last.utc);
                if reminder
                    .schedule
                    .after_close_needs_restart(consumed_through, self.clock())
                {
                    reminder.schedule.restart_after_close(now.utc)?;
                }
            }
            let due = reminder
                .schedule
                .next_after_cursor(
                    self.eligible_after
                        .get(&reminder.id)
                        .copied()
                        .unwrap_or(self.last.utc)
                        .max(self.last.utc),
                    self.clock(),
                    reminder.calendar_cursor,
                )
                .filter(|at| *at <= now.utc);
            if let Some(at) = due {
                if reminder.schedule.is_calendar_repeating() {
                    let consumed = self.clock.local_at(at).max(self.clock.local_at(now.utc));
                    reminder.calendar_cursor = Some(
                        reminder
                            .calendar_cursor
                            .map_or(consumed, |cursor| cursor.max(consumed)),
                    );
                }
                if let Some((next, _)) = reminder.schedule.after_close_next_mut() {
                    *next = None;
                }
                consumed.push(reminder.id);
                reminder.completed = reminder.schedule.ended_at(now.utc, self.clock());
                if !self
                    .toasts
                    .iter()
                    .any(|t| t.reminder_id == Some(reminder.id))
                {
                    snapshots.push(ToastSnapshot {
                        id: uuid::Uuid::new_v4(),
                        reminder_id: Some(reminder.id),
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
        snapshots.extend(self.take_due_snoozes(&mut next, now.utc, &consumed));
        if next.reminders != self.state.reminders || next.snoozed != self.state.snoozed {
            self.store.save(&next)?;
            self.state = next;
        }
        for id in consumed {
            self.skip_through(id, now.utc);
        }
        self.show_snapshots(snapshots);
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
