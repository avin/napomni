//! Monthly and yearly rules retain calendar dates; invalid dates are skipped.
use crate::reminders::Clock;
use anyhow::{Result, ensure};
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Monthly {
    pub days: Vec<u32>,
    pub times: Vec<NaiveTime>,
}
impl Monthly {
    pub fn new(mut days: Vec<u32>, mut times: Vec<NaiveTime>) -> Result<Self> {
        ensure!(
            !days.is_empty() && days.iter().all(|day| (1..=31).contains(day)),
            "Укажите числа месяца от 1 до 31"
        );
        ensure!(!times.is_empty(), "Укажите хотя бы одно время дня");
        days.sort_unstable();
        days.dedup();
        times.sort_unstable();
        times.dedup();
        Ok(Self { days, times })
    }
    pub fn next_after(
        &self,
        after: DateTime<Utc>,
        clock: &dyn Clock,
        cursor: Option<NaiveDateTime>,
    ) -> Option<DateTime<Utc>> {
        let local = clock
            .local_at(after)
            .max(cursor.unwrap_or(NaiveDateTime::MIN));
        let mut month = NaiveDate::from_ymd_opt(local.year(), local.month(), 1)?;
        loop {
            let mut next = None;
            for day in &self.days {
                if let Some(date) = NaiveDate::from_ymd_opt(month.year(), month.month(), *day) {
                    for time in &self.times {
                        if let Some(at) =
                            crate::calendar::first_after(date.and_time(*time), after, cursor, clock)
                        {
                            next =
                                Some(next.map_or(at, |previous: DateTime<Utc>| previous.min(at)));
                        }
                    }
                }
            }
            if next.is_some() {
                return next;
            }
            month = if month.month() == 12 {
                NaiveDate::from_ymd_opt(month.year().checked_add(1)?, 1, 1)?
            } else {
                NaiveDate::from_ymd_opt(month.year(), month.month() + 1, 1)?
            };
        }
    }
    pub fn description(&self) -> String {
        format!(
            "Ежемесячно по числам {}; в {} (местное время); отсутствующие даты пропускаются",
            self.days
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            format_times(&self.times)
        )
    }
}
fn format_times(times: &[NaiveTime]) -> String {
    times
        .iter()
        .map(|t| t.format("%H:%M:%S").to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YearlyDate {
    pub month: u32,
    pub day: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Yearly {
    pub dates: Vec<YearlyDate>,
    pub times: Vec<NaiveTime>,
}
impl Yearly {
    pub fn new(mut dates: Vec<YearlyDate>, mut times: Vec<NaiveTime>) -> Result<Self> {
        ensure!(
            !dates.is_empty()
                && dates
                    .iter()
                    .all(|date| NaiveDate::from_ymd_opt(2000, date.month, date.day).is_some()),
            "Укажите существующие ежегодные даты; 29 февраля допустимо"
        );
        ensure!(!times.is_empty(), "Укажите хотя бы одно время дня");
        dates.sort_unstable();
        dates.dedup();
        times.sort_unstable();
        times.dedup();
        Ok(Self { dates, times })
    }
    pub fn next_after(
        &self,
        after: DateTime<Utc>,
        clock: &dyn Clock,
        cursor: Option<NaiveDateTime>,
    ) -> Option<DateTime<Utc>> {
        if self.dates.is_empty() || self.times.is_empty() {
            return None;
        }
        let mut year = clock
            .local_at(after)
            .max(cursor.unwrap_or(NaiveDateTime::MIN))
            .year();
        while year <= NaiveDate::MAX.year() {
            let mut next = None;
            for date in &self.dates {
                if let Some(date) = NaiveDate::from_ymd_opt(year, date.month, date.day) {
                    for time in &self.times {
                        if let Some(at) =
                            crate::calendar::first_after(date.and_time(*time), after, cursor, clock)
                        {
                            next =
                                Some(next.map_or(at, |previous: DateTime<Utc>| previous.min(at)));
                        }
                    }
                }
            }
            if next.is_some() {
                return next;
            }
            year = year.checked_add(1)?;
        }
        None
    }
    pub fn description(&self) -> String {
        format!(
            "Ежегодно {}; в {} (местное время); отсутствующие даты пропускаются",
            self.dates
                .iter()
                .map(|date| format!("{:02}.{:02}", date.day, date.month))
                .collect::<Vec<_>>()
                .join(", "),
            format_times(&self.times)
        )
    }
}
