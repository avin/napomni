use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use std::sync::{Arc, Mutex};
use win_notifi_toast::{
    mutations::ReminderPatch,
    reminders::{
        Application, Clock, CommandContext, CreateReminder, JsonStore, ScheduleInput, TimeSample,
    },
};
struct TestClock(Mutex<TimeSample>);
impl TestClock {
    fn new() -> Self {
        Self(Mutex::new(TimeSample {
            utc: "2026-10-18T09:00:00Z".parse().unwrap(),
            awake_millis: 0,
        }))
    }
    fn advance(&self, seconds: i64) {
        let mut now = self.0.lock().unwrap();
        now.utc += Duration::seconds(seconds);
        now.awake_millis += (seconds * 1000) as u64;
    }
}
impl Clock for TestClock {
    fn sample(&self) -> TimeSample {
        *self.0.lock().unwrap()
    }
    fn local_at(&self, utc: DateTime<Utc>) -> NaiveDateTime {
        utc.naive_utc()
    }
    fn resolve_local(&self, local: NaiveDateTime) -> Vec<DateTime<Utc>> {
        vec![local.and_utc()]
    }
}
fn create(app: &mut Application, clock: &dyn Clock, seconds: i64) -> u64 {
    app.create(
        CreateReminder {
            text: "Одинаковый текст".into(),
            schedule: ScheduleInput::AfterSeconds { seconds },
            border: None,
            sound: None,
        },
        &CommandContext::capture(clock, "Создай", None),
    )
    .unwrap()
    .id
}
#[test]
fn edit_changes_only_named_properties_of_the_selected_id_and_persists() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let path = directory.path().join("data.json");
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let first = create(&mut app, clock.as_ref(), 60);
    let second = create(&mut app, clock.as_ref(), 120);
    let before = app.reminders().to_vec();
    let context = CommandContext::capture(clock.as_ref(), "Сделай рамку зелёной", Some(second));
    app.update(
        second,
        ReminderPatch {
            border: Some(0x00ff00),
            ..Default::default()
        },
        &context,
    )
    .unwrap();
    assert_eq!(app.reminders()[0], before[0]);
    let edited = &app.reminders()[1];
    assert_eq!(edited.text, before[1].text);
    assert_eq!(edited.schedule, before[1].schedule);
    assert_eq!(edited.sound, before[1].sound);
    assert_eq!(edited.border, 0x00ff00);
    assert!(
        app.update(
            first,
            ReminderPatch {
                sound: Some(false),
                ..Default::default()
            },
            &context
        )
        .is_err()
    );
    let app = Application::open(clock, Box::new(JsonStore::new(&path))).unwrap();
    assert_eq!(app.reminders()[1].border, 0x00ff00);
    assert!(app.reminders()[0].sound);
}
#[test]
fn edit_pause_delete_and_undo_preserve_the_open_toast_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let path = directory.path().join("data.json");
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let id = create(&mut app, clock.as_ref(), 60);
    clock.advance(60);
    app.tick().unwrap();
    let snapshot = app.open_toasts()[0].clone();
    app.update(
        id,
        ReminderPatch {
            text: Some("Новое".into()),
            border: Some(0x00ff00),
            sound: Some(false),
            ..Default::default()
        },
        &CommandContext::capture(clock.as_ref(), "Измени", Some(id)),
    )
    .unwrap();
    app.set_paused(id, true).unwrap();
    app.delete(id).unwrap();
    assert!(app.reminders().is_empty());
    assert_eq!(app.open_toasts(), std::slice::from_ref(&snapshot));
    assert!(app.signal_active());
    app.undo().unwrap();
    assert_eq!(app.reminders()[0].id, id);
    assert!(app.reminders()[0].paused);
    assert_eq!(app.open_toasts(), &[snapshot]);
    app.close_toast(id);
    assert!(!app.signal_active());
    assert!(!app.can_undo());
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
}

#[test]
fn undo_covers_creation_edit_and_pause_and_restores_deleted_id_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let path = directory.path().join("data.json");
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let removed = create(&mut app, clock.as_ref(), 60);
    app.undo().unwrap();
    assert!(app.reminders().is_empty());
    let id = create(&mut app, clock.as_ref(), 60);
    assert!(id > removed);
    let original = app.reminders()[0].clone();
    let context = CommandContext::capture(clock.as_ref(), "Измени", Some(id));
    app.update(
        id,
        ReminderPatch {
            text: Some("Другое".into()),
            ..Default::default()
        },
        &context,
    )
    .unwrap();
    app.undo().unwrap();
    assert_eq!(app.reminders()[0], original);
    app.set_paused(id, true).unwrap();
    app.undo().unwrap();
    assert!(!app.reminders()[0].paused);
    app.delete(id).unwrap();
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    assert!(app.can_undo());
    app.undo().unwrap();
    assert_eq!(app.reminders()[0], original);
    assert!(!app.can_undo());
    clock.advance(60);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, id);
}

#[test]
fn pause_resume_and_late_undo_skip_missed_occurrences_without_skipping_other_records() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let paused = create(&mut app, clock.as_ref(), 60);
    let deleted = create(&mut app, clock.as_ref(), 60);
    let other = create(&mut app, clock.as_ref(), 120);
    app.set_paused(paused, true).unwrap();
    app.delete(deleted).unwrap();
    clock.advance(90);
    app.undo().unwrap();
    app.set_paused(paused, false).unwrap();
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    assert!(
        app.reminders()
            .iter()
            .filter(|r| r.id != other)
            .all(|r| r.completed)
    );
    clock.advance(30);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert_eq!(app.open_toasts()[0].reminder_id, other);
}

#[test]
fn invalid_edit_does_not_change_data_or_consume_undo_and_time_edit_keeps_other_properties() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let id = create(&mut app, clock.as_ref(), 60);
    let context = CommandContext::capture(clock.as_ref(), "Измени", Some(id));
    assert!(
        app.update(
            id,
            ReminderPatch {
                text: Some(" ".into()),
                ..Default::default()
            },
            &context
        )
        .is_err()
    );
    app.undo().unwrap();
    assert!(app.reminders().is_empty());
    let id = create(&mut app, clock.as_ref(), 60);
    let original = app.reminders()[0].clone();
    app.update(
        id,
        ReminderPatch {
            schedule: Some(ScheduleInput::AfterSeconds { seconds: 120 }),
            ..Default::default()
        },
        &CommandContext::capture(clock.as_ref(), "Через 2 минуты", Some(id)),
    )
    .unwrap();
    assert_eq!(app.reminders()[0].text, original.text);
    assert_eq!(app.reminders()[0].border, original.border);
    assert_eq!(app.reminders()[0].sound, original.sound);
    clock.advance(60);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(60);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
}

#[test]
fn recurring_resume_and_undo_skip_elapsed_grid_points_only_for_the_affected_record() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let id = app
        .create(
            CreateReminder {
                text: "Каждую минуту".into(),
                schedule: ScheduleInput::Interval {
                    seconds: 60,
                    start_local: None,
                },
                border: None,
                sound: None,
            },
            &context,
        )
        .unwrap()
        .id;
    let other = app
        .create(
            CreateReminder {
                text: "Другой".into(),
                schedule: ScheduleInput::Interval {
                    seconds: 120,
                    start_local: None,
                },
                border: None,
                sound: None,
            },
            &context,
        )
        .unwrap()
        .id;
    clock.advance(60);
    app.tick().unwrap();
    let snapshot = app.open_toasts()[0].clone();
    app.update(
        id,
        ReminderPatch {
            sound: Some(false),
            ..Default::default()
        },
        &CommandContext::capture(clock.as_ref(), "Без звука", Some(id)),
    )
    .unwrap();
    assert_eq!(app.open_toasts(), &[snapshot]);
    app.close_toast(id);
    clock.advance(75);
    app.set_paused(id, true).unwrap();
    app.undo().unwrap();
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert_eq!(app.open_toasts()[0].reminder_id, other);
    clock.advance(45);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, id);
    assert!(!app.open_toasts()[0].sound);
    app.delete(id).unwrap();
    app.close_toast(id);
    clock.advance(70);
    app.undo().unwrap();
    app.tick().unwrap();
    assert!(app.open_toasts().iter().all(|t| t.reminder_id != id));
    clock.advance(50);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, id);
}

#[test]
fn editing_time_after_clock_rollback_allows_the_new_future_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let id = create(&mut app, clock.as_ref(), 60);
    clock.advance(60);
    app.tick().unwrap();
    app.close_toast(id);
    clock.0.lock().unwrap().utc -= Duration::seconds(60);
    app.tick().unwrap();
    app.update(
        id,
        ReminderPatch {
            schedule: Some(ScheduleInput::AfterSeconds { seconds: 30 }),
            ..Default::default()
        },
        &CommandContext::capture(clock.as_ref(), "Через30 секунд", Some(id)),
    )
    .unwrap();
    clock.advance(30);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
}

#[test]
fn undo_of_calendar_property_edit_keeps_occurrences_consumed_since_the_edit() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let path = directory.path().join("data.json");
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let schedule=serde_json::from_value(serde_json::json!({"kind":"weekly","weekdays":[7],"times":["09:01:00","09:02:00"],"every_n_weeks":1})).unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let id = app
        .create(
            CreateReminder {
                text: "Календарное".into(),
                schedule,
                border: None,
                sound: None,
            },
            &context,
        )
        .unwrap()
        .id;
    app.update(
        id,
        ReminderPatch {
            sound: Some(false),
            ..Default::default()
        },
        &CommandContext::capture(clock.as_ref(), "Без звука", Some(id)),
    )
    .unwrap();
    for _ in 0..2 {
        clock.advance(60);
        app.tick().unwrap();
        assert_eq!(app.open_toasts().len(), 1);
        app.close_toast(id);
    }
    app.undo().unwrap();
    assert!(app.reminders()[0].sound);
    clock.0.lock().unwrap().utc -= Duration::minutes(2);
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    for _ in 0..2 {
        clock.advance(60);
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
    }
}
