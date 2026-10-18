//! Local-calendar weekly rules. Week phase uses Monday dates, never UTC durations.
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Datelike, Days, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};

use crate::reminders::Clock;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weekly {
    pub weekdays: Vec<u32>,
    pub times: Vec<NaiveTime>,
    pub every_n_weeks: u32,
    pub anchor_monday: NaiveDate,
}

impl Weekly {
    pub fn new(
        mut weekdays: Vec<u32>,
        mut times: Vec<NaiveTime>,
        every_n_weeks: u32,
        anchor_monday: Option<NaiveDate>,
        submitted_date: NaiveDate,
    ) -> Result<Self> {
        ensure!(every_n_weeks > 0, "Число недель должно быть положительным");
        ensure!(
            !weekdays.is_empty() && weekdays.iter().all(|day| (1..=7).contains(day)),
            "Укажите дни недели от 1 (понедельник) до 7 (воскресенье)"
        );
        ensure!(!times.is_empty(), "Укажите хотя бы одно время дня");
        ensure!(
            every_n_weeks == 1 || anchor_monday.is_some(),
            "Уточните начальную неделю цикла каждые N недель"
        );
        let anchor_monday = anchor_monday.unwrap_or(
            submitted_date
                .checked_sub_days(Days::new(
                    submitted_date.weekday().num_days_from_monday() as u64
                ))
                .context("Дата начала недели вне допустимого диапазона")?,
        );
        ensure!(
            anchor_monday.weekday().num_days_from_monday() == 0,
            "Начальная неделя задаётся датой её понедельника"
        );
        weekdays.sort_unstable();
        weekdays.dedup();
        times.sort_unstable();
        times.dedup();
        Ok(Self {
            weekdays,
            times,
            every_n_weeks,
            anchor_monday,
        })
    }

    pub fn next_after(
        &self,
        after: DateTime<Utc>,
        clock: &dyn Clock,
        cursor: Option<chrono::NaiveDateTime>,
    ) -> Option<DateTime<Utc>> {
        if self.every_n_weeks == 0
            || self.times.is_empty()
            || self.weekdays.is_empty()
            || self.weekdays.iter().any(|day| !(1..=7).contains(day))
        {
            return None;
        }
        let local_date = clock
            .local_at(after)
            .max(cursor.unwrap_or(chrono::NaiveDateTime::MIN))
            .date();
        let monday = local_date
            .checked_sub_days(Days::new(local_date.weekday().num_days_from_monday() as u64))?;
        let elapsed_weeks = (monday - self.anchor_monday).num_days() / 7;
        let cycle = i64::from(self.every_n_weeks);
        let active_week = if elapsed_weeks < 0 {
            0
        } else {
            elapsed_weeks + (cycle - elapsed_weeks % cycle) % cycle
        };
        let mut monday = self
            .anchor_monday
            .checked_add_days(Days::new(u64::try_from(active_week).ok()?.checked_mul(7)?))?;
        loop {
            let mut next = None;
            for weekday in &self.weekdays {
                let date =
                    monday.checked_add_days(Days::new(u64::from(weekday.checked_sub(1)?)))?;
                for time in &self.times {
                    // Pick the FIRST instant before filtering by `after`. Filtering both
                    // fold candidates first would replay a repeated local time later.
                    if let Some(at) =
                        crate::calendar::first_after(date.and_time(*time), after, cursor, clock)
                    {
                        next = Some(next.map_or(at, |previous: DateTime<Utc>| previous.min(at)));
                    }
                }
            }
            if next.is_some() {
                return next;
            }
            monday = monday
                .checked_add_days(Days::new(u64::from(self.every_n_weeks).checked_mul(7)?))?;
        }
    }

    pub fn description(&self) -> String {
        const DAYS: [&str; 7] = ["пн", "вт", "ср", "чт", "пт", "сб", "вс"];
        let days = self
            .weekdays
            .iter()
            .filter_map(|day| day.checked_sub(1).and_then(|i| DAYS.get(i as usize)))
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        let times = self
            .times
            .iter()
            .map(|time| time.format("%H:%M:%S").to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Каждые {} нед.: {}; в {} (местное время), начальная неделя с {}",
            self.every_n_weeks,
            days,
            times,
            self.anchor_monday.format("%d.%m.%Y")
        )
    }
}
