use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use napomni::reminders::{Clock, TimeSample};
use napomni::{
    external_commands::execute,
    reminder_tools::execute_tool,
    reminders::{Application, CommandContext, JsonStore, SystemClock},
};
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;

#[test]
fn committed_reminder_result_survives_a_desktop_adapter_failure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reminders.json");
    let mut app =
        Application::open(Arc::new(SystemClock), Box::new(JsonStore::new(&path))).unwrap();
    let result = execute(
        &mut app,
        "create",
        &json!({"text":"Committed", "schedule":{"kind":"after_seconds","seconds":3600}}),
    )
    .unwrap();
    let result = napomni::external_commands::with_desktop_result(
        "create",
        result,
        Err(anyhow::anyhow!("Native display unavailable")),
    )
    .unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(result["reminder"]["text"], "Committed");
    assert_eq!(result["warnings"].as_array().unwrap().len(), 1);
    drop(app);
    let mut app =
        Application::open(Arc::new(SystemClock), Box::new(JsonStore::new(&path))).unwrap();
    assert_eq!(app.reminders()[0].text, "Committed");
    execute(&mut app, "undo", &json!({})).unwrap();
    assert!(app.reminders().is_empty());
}

#[test]
fn notification_adapter_failure_reports_uncertainty_for_an_already_created_card() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = Application::open(
        Arc::new(SystemClock),
        Box::new(JsonStore::new(directory.path().join("reminders.json"))),
    )
    .unwrap();
    let result = execute(&mut app, "notify", &json!({"text":"Possibly shown"})).unwrap();
    let error = napomni::external_commands::with_desktop_result(
        "notify",
        result,
        Err(anyhow::anyhow!("Native placement failed")),
    )
    .unwrap_err();
    assert!(error.to_string().contains("may already exist"));
    assert_eq!(app.open_toasts().len(), 1);
}

struct TestClock(Mutex<TimeSample>);
impl TestClock {
    fn new() -> Self {
        Self(Mutex::new(TimeSample {
            utc: "2026-10-23T09:00:00Z".parse().unwrap(),
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

#[test]
fn external_create_preserves_literal_quoted_text_and_shares_chat_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = Application::open(
        Arc::new(SystemClock),
        Box::new(JsonStore::new(dir.path().join("reminders.json"))),
    )
    .unwrap();
    let created = execute(&mut app, "create", &json!({"text":"До \"кавычек\"\nпосле", "schedule":{"kind":"after_seconds","seconds":3600}})).unwrap();
    let context = CommandContext::capture(app.clock(), "chat", None);
    let listed = execute_tool(&mut app, "list_reminders", "{}", &context).unwrap();
    assert_eq!(listed["reminders"][0]["reminder"], created["reminder"]);
    assert_eq!(created["reminder"]["text"], "До \"кавычек\"\nпосле");
    execute_tool(&mut app, "undo_last_operation", "{}", &context).unwrap();
    assert_eq!(
        execute(&mut app, "list", &json!({})).unwrap()["reminders"],
        json!([])
    );
}

#[test]
fn external_update_of_chat_record_preserves_other_properties_records_and_undo_on_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = Application::open(
        Arc::new(SystemClock),
        Box::new(JsonStore::new(dir.path().join("reminders.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(app.clock(), "chat", None);
    for border in [0x123456, 0x654321] {
        execute_tool(&mut app, "create_reminder", &json!({"text":"Same", "schedule":{"kind":"interval","seconds":3600},"border":border,"sound":false}).to_string(), &context).unwrap();
    }
    let original = app.reminders().to_vec();
    let id = original[0].id;
    let changed = execute(&mut app, "update", &json!({"id":id,"patch":{"text":"New"}})).unwrap();
    assert_eq!(changed["reminder"]["text"], "New");
    assert_eq!(app.reminders()[0].schedule, original[0].schedule);
    assert_eq!(app.reminders()[0].border, 0x123456);
    assert!(!app.reminders()[0].sound);
    assert_eq!(app.reminders()[1], original[1]);
    for arguments in [
        json!({"id":99999,"patch":{"text":"Missing"}}),
        json!({"id":id,"patch":{}}),
        json!({"id":id,"patch":{"bogus":true}}),
    ] {
        assert!(execute(&mut app, "update", &arguments).is_err());
    }
    execute(&mut app, "undo", &json!({})).unwrap();
    assert_eq!(app.reminders(), original);
}

#[test]
fn external_pause_resume_delete_and_undo_keep_open_snapshot_and_saved_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reminders.json");
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let created = execute(
        &mut app,
        "create",
        &json!({"text":"Original", "schedule":{"kind":"interval","seconds":60}}),
    )
    .unwrap();
    let id = created["reminder"]["id"].clone();
    clock.advance(60);
    app.tick().unwrap();
    let snapshot = app.open_toasts()[0].clone();
    execute(
        &mut app,
        "update",
        &json!({"id":id,"patch":{"text":"Changed","border":0xff0000,"sound":false}}),
    )
    .unwrap();
    execute(&mut app, "pause", &json!({"id":id})).unwrap();
    assert!(app.reminders()[0].paused);
    clock.advance(3600);
    execute(&mut app, "resume", &json!({"id":id})).unwrap();
    assert!(!app.reminders()[0].paused);
    execute(&mut app, "delete", &json!({"id":id})).unwrap();
    assert!(app.reminders().is_empty());
    execute(&mut app, "undo", &json!({})).unwrap();
    assert_eq!(app.open_toasts(), &[snapshot]);
    assert!(app.signal_active());
    let restored = app.reminders()[0].clone();
    drop(app);
    let app = Application::open(clock, Box::new(JsonStore::new(&path))).unwrap();
    assert_eq!(app.reminders(), &[restored]);
}

#[test]
fn external_save_failure_preserves_committed_records_and_shared_undo() {
    use napomni::reminders::{Persisted, ReminderStore};
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Store {
        json: JsonStore,
        fail: Arc<AtomicBool>,
    }
    impl ReminderStore for Store {
        fn load(&self) -> anyhow::Result<Persisted> {
            self.json.load()
        }
        fn save(&self, state: &Persisted) -> anyhow::Result<()> {
            anyhow::ensure!(!self.fail.load(Ordering::SeqCst), "Disk unavailable");
            self.json.save(state)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let fail = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(Store {
            json: JsonStore::new(&path),
            fail: fail.clone(),
        }),
    )
    .unwrap();
    let record = execute(
        &mut app,
        "create",
        &json!({"text":"Saved", "schedule":{"kind":"interval","seconds":60}}),
    )
    .unwrap();
    let id = record["reminder"]["id"].clone();
    execute(
        &mut app,
        "update",
        &json!({"id":id,"patch":{"text":"Committed"}}),
    )
    .unwrap();
    fail.store(true, Ordering::SeqCst);
    for (command, args) in [
        (
            "create",
            json!({"text":"Lost","schedule":{"kind":"after_seconds","seconds":60}}),
        ),
        ("update", json!({"id":id,"patch":{"text":"Lost"}})),
        ("pause", json!({"id":id})),
        ("delete", json!({"id":id})),
        ("undo", json!({})),
    ] {
        assert!(execute(&mut app, command, &args).is_err());
        assert_eq!(
            execute(&mut app, "list", &json!({})).unwrap()["reminders"][0]["reminder"]["text"],
            "Committed"
        );
    }
    drop(app);
    let mut app = Application::open(clock, Box::new(JsonStore::new(&path))).unwrap();
    assert_eq!(app.reminders()[0].text, "Committed");
    execute(&mut app, "undo", &json!({})).unwrap();
    assert_eq!(app.reminders()[0].text, "Saved");
}

#[test]
fn external_commands_accept_every_existing_schedule_and_validate_limits() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock,
        Box::new(JsonStore::new(dir.path().join("state.json"))),
    )
    .unwrap();
    let schedules = vec![
        json!({"kind":"after_seconds","seconds":60}),
        json!({"kind":"at_local","date_time":"2026-11-01T10:00:00"}),
        json!({"kind":"tomorrow","time":"10:00:00"}),
        json!({"kind":"interval","seconds":60,"start_local":"2026-11-01T10:00:00"}),
        json!({"kind":"interval_after_close","seconds":60}),
        json!({"kind":"weekly","weekdays":[1,3],"times":["10:00:00"],"every_n_weeks":2,"anchor_monday":"2026-10-19"}),
        json!({"kind":"monthly","days":[1,31],"times":["10:00:00"]}),
        json!({"kind":"yearly","dates":[{"month":2,"day":29}],"times":["10:00:00"]}),
        json!({"kind":"constrained","base":{"kind":"interval","seconds":3600},"limits":{"start_date":"2026-10-24","end_date":"2026-11-01","window":{"start":"09:00:00","end":"17:00:00"},"weekdays":[1,2,3,4,5],"excluded_dates":["2026-10-26"]}}),
    ];
    for schedule in &schedules {
        let record = execute(
            &mut app,
            "create",
            &json!({"text":"Schedule", "schedule":schedule}),
        )
        .unwrap();
        assert!(
            record["schedule_description"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        );
    }
    assert_eq!(
        execute(&mut app, "list", &json!({})).unwrap()["reminders"]
            .as_array()
            .unwrap()
            .len(),
        9
    );
    let before = app.reminders().to_vec();
    assert!(
        execute(
            &mut app,
            "create",
            &json!({"text":"Invalid","schedule":{"kind":"interval","seconds":0}})
        )
        .is_err()
    );
    assert_eq!(app.reminders(), before);
}
