//! Constant-time interval grid arithmetic. The anchor is the first eligible occurrence.
use crate::reminders::{Clock, Schedule};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
pub(crate) fn resolve(
    seconds: i64,
    start_local: Option<NaiveDateTime>,
    clock: &dyn Clock,
) -> Result<Schedule> {
    ensure!(seconds > 0, "Интервал должен быть положительным");
    let duration = Duration::try_seconds(seconds).context("Слишком большой интервал")?;
    let anchor_utc = match start_local {
        Some(local) => crate::reminders::resolve_unique(clock, local)?,
        None => clock
            .sample()
            .utc
            .checked_add_signed(duration)
            .context("Слишком большой интервал")?,
    };
    Ok(Schedule::Interval {
        anchor_utc,
        period_seconds: seconds,
    })
}
pub(crate) fn next_after(
    anchor: DateTime<Utc>,
    seconds: i64,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if seconds <= 0 {
        return None;
    }
    if after < anchor {
        return Some(anchor);
    }
    let steps = (after - anchor)
        .num_seconds()
        .checked_div(seconds)?
        .checked_add(1)?;
    anchor.checked_add_signed(Duration::try_seconds(steps.checked_mul(seconds)?)?)
}
pub(crate) fn description(anchor: DateTime<Utc>, seconds: i64, clock: &dyn Clock) -> String {
    let (amount, unit) = if seconds % 3600 == 0 {
        (seconds / 3600, "ч")
    } else if seconds % 60 == 0 {
        (seconds / 60, "мин")
    } else {
        (seconds, "с")
    };
    format!(
        "Каждые {amount} {unit}, первая точка сетки {} (местное время)",
        clock.local_at(anchor).format("%d.%m.%Y в %H:%M:%S")
    )
}
