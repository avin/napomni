#![cfg(windows)]
use serde_json::Value;
use std::process::{Command, Output};

#[path = "support/desktop.rs"]
mod desktop;
use desktop::DesktopInstance as Instance;

impl Instance {
    fn list(&mut self) -> Value {
        let output = self.command().arg("list").output().unwrap();
        let result = success(output);
        self.pid = Some(result["host_pid"].as_u64().unwrap() as u32);
        result
    }
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn cli_cold_launch_then_reuses_the_same_isolated_owner() {
    let mut instance = Instance::new();
    let first = instance.list();
    assert_eq!(first["reminders"], serde_json::json!([]));
    assert_eq!(instance.list()["host_pid"], first["host_pid"]);
    // Observe real native windows rather than inferring visibility from core state.
    use windows::Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible},
    };
    unsafe extern "system" fn inspect(window: HWND, state: LPARAM) -> windows::core::BOOL {
        let state = unsafe { &mut *(state.0 as *mut (u32, bool)) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut pid));
            if pid == state.0 && IsWindowVisible(window).as_bool() {
                state.1 = true;
            }
        }
        true.into()
    }
    let mut state = (instance.pid.unwrap(), false);
    unsafe {
        EnumWindows(
            Some(inspect),
            LPARAM((&mut state as *mut (u32, bool)) as isize),
        )
        .unwrap();
    }
    assert!(
        !state.1,
        "Background CLI launch opened a visible native window"
    );
}

#[test]
fn simultaneous_cold_clients_and_repeated_gui_launch_share_one_owner() {
    let mut instance = Instance::new();
    let clients = (0..6)
        .map(|_| {
            let mut command = instance.command();
            std::thread::spawn(move || success(command.arg("list").output().unwrap()))
        })
        .collect::<Vec<_>>();
    let results = clients
        .into_iter()
        .map(|client| client.join().unwrap())
        .collect::<Vec<_>>();
    instance.pid = Some(results[0]["host_pid"].as_u64().unwrap() as u32);
    for result in &results {
        assert_eq!(result["host_pid"], results[0]["host_pid"]);
    }
    let status = Command::new(
        instance
            .dir
            .path()
            .join("installed app with spaces/napomni.exe"),
    )
    .env("NAPOMNI_DATA_DIR", instance.dir.path())
    .status()
    .unwrap();
    assert!(status.success());
    assert_eq!(instance.list()["host_pid"], results[0]["host_pid"]);
}

#[test]
fn invalid_command_and_failed_launch_return_diagnostics_without_success() {
    let instance = Instance::new();
    let invalid = instance.command().arg("nonsense").output().unwrap();
    assert!(!invalid.status.success());
    assert!(!invalid.stderr.is_empty());
    let missing = instance
        .command()
        .env(
            "NAPOMNI_APP_EXE",
            instance.dir.path().join("missing app.exe"),
        )
        .arg("list")
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(!missing.stderr.is_empty());
    let help = instance.command().arg("--help").output().unwrap();
    assert!(help.status.success());
}

#[test]
fn unavailable_application_returns_within_the_documented_timeout() {
    let instance = Instance::new();
    // A real executable which exits without hosting IPC: launch itself succeeds.
    let started = std::time::Instant::now();
    let output = instance
        .command()
        .env(
            "NAPOMNI_APP_EXE",
            instance
                .dir
                .path()
                .join("installed app with spaces/napomni-cli.exe"),
        )
        .arg("list")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
}

#[test]
fn cli_lists_actual_chat_records_with_unicode_quotes_and_newlines() {
    use napomni::{
        reminder_tools::execute_tool,
        reminders::{Application, CommandContext, JsonStore, SystemClock},
    };
    use std::sync::Arc;
    let mut instance = Instance::new();
    let id = {
        let mut app = Application::open(
            Arc::new(SystemClock),
            Box::new(JsonStore::new(instance.dir.path().join("reminders.json"))),
        )
        .unwrap();
        let context = CommandContext::capture(app.clock(), "chat", None);
        let result = execute_tool(&mut app, "create_reminder", &serde_json::json!({"text":"Кириллица \"кавычки\"\nстрока", "schedule":{"kind":"after_seconds","seconds":3600}}).to_string(), &context).unwrap();
        result["reminder"]["id"].clone()
    };
    let list = instance.list();
    assert_eq!(list["reminders"][0]["reminder"]["id"], id);
    assert_eq!(
        list["reminders"][0]["reminder"]["text"],
        "Кириллица \"кавычки\"\nстрока"
    );
    assert!(list["reminders"][0]["schedule_description"].is_string());
    assert!(list["reminders"][0].get("schedule_status").is_some());
}
