//! Common local-calendar limits. Bounds include endpoints; the base phase is unchanged.
use crate::reminders::{Clock, Schedule};
use anyhow::{Result, ensure};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeWindow {
    pub start: NaiveTime,
    pub end: NaiveTime,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraints {
    #[serde(default)]
    pub start_date: Option<NaiveDate>,
    #[serde(default)]
    pub end_date: Option<NaiveDate>,
    #[serde(default)]
    pub window: Option<TimeWindow>,
    #[serde(default)]
    pub weekdays: Option<Vec<u32>>,
    #[serde(default)]
    pub excluded_dates: Vec<NaiveDate>,
}
impl Constraints {
    pub(crate) fn apply_to(
        self,
        schedule: Schedule,
        now: DateTime<Utc>,
        clock: &dyn Clock,
    ) -> Result<Schedule> {
        let base = match schedule {
            Schedule::Constrained { base, .. } => *base,
            base => base,
        };
        self.validate(&base)?;
        let schedule = Schedule::Constrained {
            base: Box::new(base),
            limits: self,
        };
        let mut probe = schedule.clone();
        if let Some((next, _)) = probe.after_close_next_mut()
            && next.is_none()
        {
            probe.restart_after_close(now)?;
        }
        ensure!(
            probe.next_after(now, clock).is_some(),
            "Сочетание ограничений не даёт будущего срабатывания в поддерживаемом поиске: до 400 календарных лет и 100000 кандидатов. Уточните правило"
        );
        Ok(schedule)
    }
    pub(crate) fn validate(&self, base: &Schedule) -> Result<()> {
        ensure!(
            !matches!(
                base,
                Schedule::Once { .. } | Schedule::OnceLocal { .. } | Schedule::Constrained { .. }
            ),
            "Ограничения поддерживаются для одного повторяющегося правила; вложенные ограничения и однократные даты не поддерживаются"
        );
        ensure!(
            !matches!((self.start_date,self.end_date),(Some(start),Some(end)) if start>end),
            "Начальная дата должна быть не позже конечной"
        );
        ensure!(
            self.window
                .as_ref()
                .is_none_or(|window| window.start <= window.end),
            "Окно через полночь пока не поддерживается; начало должно быть не позже конца"
        );
        ensure!(self.weekdays.as_ref().is_none_or(|days|!days.is_empty()&&days.iter().all(|day|(1..=7).contains(day))),"Укажите дни недели от 1 до 7; будни означают понедельник–пятницу");
        let times = match base {
            Schedule::Weekly { rule } => Some(&rule.times),
            Schedule::Monthly { rule } => Some(&rule.times),
            Schedule::Yearly { rule } => Some(&rule.times),
            _ => None,
        };
        if let (Some(times), Some(window)) = (times, self.window.as_ref()) {
            ensure!(
                times
                    .iter()
                    .any(|time| *time >= window.start && *time <= window.end),
                "Ни одно время календарного правила не входит в заданное окно"
            );
        }
        if let (Schedule::Weekly { rule }, Some(weekdays)) = (base, self.weekdays.as_ref()) {
            ensure!(
                rule.weekdays.iter().any(|day| weekdays.contains(day)),
                "Дни недельного правила и ограничения не пересекаются"
            );
        }
        Ok(())
    }
    fn allowed_date(&self, date: NaiveDate) -> bool {
        self.start_date.is_none_or(|start| date >= start)
            && self.end_date.is_none_or(|end| date <= end)
            && self
                .weekdays
                .as_ref()
                .is_none_or(|days| days.contains(&date.weekday().number_from_monday()))
            && !self.excluded_dates.contains(&date)
    }
    fn allowed(&self, local: NaiveDateTime) -> bool {
        self.allowed_date(local.date())
            && self
                .window
                .as_ref()
                .is_none_or(|window| local.time() >= window.start && local.time() <= window.end)
    }
    fn next_boundary(&self, local: NaiveDateTime, horizon: NaiveDate) -> Option<NaiveDateTime> {
        let mut date = local.date().max(self.start_date.unwrap_or(NaiveDate::MIN));
        let start = self
            .window
            .as_ref()
            .map_or(NaiveTime::MIN, |window| window.start);
        if date == local.date() && self.allowed_date(date) && local.time() < start {
            return Some(date.and_time(start));
        }
        if date == local.date() {
            date = date.succ_opt()?;
        }
        while date <= horizon && self.end_date.is_none_or(|end| date <= end) {
            if self.allowed_date(date) {
                return Some(date.and_time(start));
            }
            date = date.succ_opt()?;
        }
        None
    }
    pub(crate) fn next_after(
        &self,
        base: &Schedule,
        after: DateTime<Utc>,
        clock: &dyn Clock,
        cursor: Option<NaiveDateTime>,
    ) -> Option<DateTime<Utc>> {
        let start = clock
            .local_at(after)
            .date()
            .max(self.start_date.unwrap_or(NaiveDate::MIN));
        let horizon = NaiveDate::from_ymd_opt(start.year().checked_add(400)?, 12, 31)
            .unwrap_or(NaiveDate::MAX);
        let mut after = after;
        for _ in 0..100_000 {
            let candidate = base.next_after_cursor(after, clock, cursor)?;
            let local = clock.local_at(candidate);
            if local.date() > horizon || self.end_date.is_some_and(|end| local.date() > end) {
                return None;
            }
            if self.allowed(local) {
                return Some(candidate);
            }
            let boundary = self.next_boundary(local, horizon)?;
            // Jump the search boundary, then ask the ORIGINAL rule again. This
            // filters occurrences without moving their interval/week phase.
            after = clock
                .resolve_local(boundary)
                .into_iter()
                .filter(|at| *at > candidate)
                .min()
                .and_then(|at| at.checked_sub_signed(Duration::nanoseconds(1)))
                .unwrap_or(candidate);
        }
        None
    }
    pub(crate) fn ended_at(&self, now: DateTime<Utc>, clock: &dyn Clock) -> bool {
        self.end_date
            .is_some_and(|end| clock.local_at(now).date() > end)
    }
    pub(crate) fn description(&self) -> String {
        let mut parts = Vec::new();
        if let Some(date) = self.start_date {
            parts.push(format!("с {} включительно", date.format("%d.%m.%Y")));
        }
        if let Some(date) = self.end_date {
            parts.push(format!("по {} включительно", date.format("%d.%m.%Y")));
        }
        if let Some(window) = &self.window {
            parts.push(format!(
                "часы {}–{} включительно",
                window.start.format("%H:%M:%S"),
                window.end.format("%H:%M:%S")
            ));
        }
        if let Some(days) = &self.weekdays {
            const DAYS: [&str; 7] = ["пн", "вт", "ср", "чт", "пт", "сб", "вс"];
            parts.push(format!(
                "дни {}",
                days.iter()
                    .filter_map(|day| day.checked_sub(1).and_then(|day| DAYS.get(day as usize)))
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !self.excluded_dates.is_empty() {
            parts.push(format!(
                "кроме {}",
                self.excluded_dates
                    .iter()
                    .map(|date| date.format("%d.%m.%Y").to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if parts.is_empty() {
            "Дополнительных ограничений нет".into()
        } else {
            format!("Ограничения (местное время): {}", parts.join("; "))
        }
    }
}

pub(crate) fn schema() -> serde_json::Value {
    serde_json::json!({"type":"object","properties":{
  "start_date":{"type":"string","description":"Inclusive start date in local time, YYYY-MM-DD"},
  "end_date":{"type":"string","description":"Inclusive end date in local time, YYYY-MM-DD"},
  "window":{"type":"object","properties":{"start":{"type":"string","description":"Inclusive local time window start, HH:MM:SS"},"end":{"type":"string","description":"Inclusive local time window end, HH:MM:SS; must not precede the start"}},"required":["start","end"],"additionalProperties":false},
  "weekdays":{"type":"array","items":{"type":"integer","minimum":1,"maximum":7},"minItems":1,"description":"Working days: 1-5, Monday-Friday; no holiday calendar"},
  "excluded_dates":{"type":"array","items":{"type":"string"},"description":"Excluded local dates, YYYY-MM-DD; no rescheduling or interval phase shifts"}
 },"additionalProperties":false,"description":"Local recurrence constraints. Bounds are inclusive. An empty object removes constraints without changing the base recurrence phase."})
}
