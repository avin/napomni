//! Shared local-calendar occurrence policy for weekly, monthly and yearly rules.
use chrono::{DateTime, NaiveDateTime, Utc};

use crate::reminders::Clock;

/// Skips nonexistent wall times and admits the first instant of a repeated wall
/// time only. A persisted local cursor also survives manual clock rollback.
pub(crate) fn first_after(
    local: NaiveDateTime,
    after: DateTime<Utc>,
    cursor: Option<NaiveDateTime>,
    clock: &dyn Clock,
) -> Option<DateTime<Utc>> {
    if cursor.is_some_and(|cursor| local <= cursor) {
        return None;
    }
    clock
        .resolve_local(local)
        .into_iter()
        .min()
        .filter(|at| *at > after)
}
