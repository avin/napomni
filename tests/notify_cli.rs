#![cfg(windows)]
use serde_json::Value;
use std::process::{Command, Output};

fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn real_cli_transports_the_exact_text_to_the_shared_application_handler() {
    let dir = tempfile::tempdir().unwrap();
    let mut host = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fixture_notify_host", "--ignored", "--nocapture"])
        .env("NAPOMNI_DATA_DIR", dir.path())
        .env("NAPOMNI_NOTIFY_FIXTURE", "1")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    while !dir.path().join("ready").exists() {
        assert!(host.try_wait().unwrap().is_none(), "fixture host exited");
        assert!(
            started.elapsed().as_secs() < 10,
            "fixture host did not become ready"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let text = "Кириллица: \"quotes\"
second line";
    let output = Command::new(env!("CARGO_BIN_EXE_napomni-cli"))
        .args(["notify", text])
        .env("NAPOMNI_DATA_DIR", dir.path())
        .output()
        .unwrap();
    std::fs::write(dir.path().join("client_finished"), "done").unwrap();
    let status = host.wait().unwrap();
    let result = success(output);
    assert!(status.success());
    assert_eq!(result["ok"], true);
    assert!(result.get("id").is_none());
    assert!(result.get("notification_id").is_none());
    let snapshot: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("observed.json")).unwrap()).unwrap();
    assert_eq!(snapshot["text"], text);
    assert_eq!(snapshot["reminder_id"], Value::Null);
}

// Separate test process owns environment and IPC, never the user's app.
#[test]
#[ignore = "private child-process fixture, invoked by the transport test"]
fn fixture_notify_host() {
    if std::env::var_os("NAPOMNI_NOTIFY_FIXTURE").is_none() {
        return;
    }
    use napomni::{
        external_commands::execute,
        local_host,
        reminders::{Application, JsonStore, SystemClock},
    };
    let directory = std::path::PathBuf::from(std::env::var_os("NAPOMNI_DATA_DIR").unwrap());
    let owner = local_host::claim_owner().unwrap().unwrap();
    let mut app = Application::open(
        std::sync::Arc::new(SystemClock),
        Box::new(JsonStore::new(directory.join("reminders.json"))),
    )
    .unwrap();
    let pending = local_host::serve().unwrap();
    std::fs::write(directory.join("ready"), "ready").unwrap();
    let command = pending
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let result = execute(
        &mut app,
        &command.request.command,
        &command.request.arguments,
    )
    .unwrap();
    std::fs::write(
        directory.join("observed.json"),
        serde_json::to_vec(&app.open_toasts()[0]).unwrap(),
    )
    .unwrap();
    command.reply.send(result).unwrap();
    // Keep the real channel alive until the CLI has received and consumed its reply.
    let started = std::time::Instant::now();
    while !directory.join("client_finished").exists() {
        assert!(started.elapsed().as_secs() < 10, "client did not finish");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    drop(owner);
}

struct DesktopInstance {
    dir: tempfile::TempDir,
    pid: Option<u32>,
}
impl DesktopInstance {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("installed app with spaces");
        std::fs::create_dir(&bin).unwrap();
        for (source, name) in [
            (env!("CARGO_BIN_EXE_napomni"), "napomni.exe"),
            (env!("CARGO_BIN_EXE_napomni-cli"), "napomni-cli.exe"),
        ] {
            std::fs::copy(source, bin.join(name)).unwrap();
        }
        Self { dir, pid: None }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(
            self.dir
                .path()
                .join("installed app with spaces/napomni-cli.exe"),
        );
        command
            .env("NAPOMNI_DATA_DIR", self.dir.path())
            .env_remove("NAPOMNI_APP_EXE");
        command
    }
}
impl Drop for DesktopInstance {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output();
        }
    }
}

#[test]
fn notify_cold_launches_a_visible_native_toast_without_showing_or_focusing_main() {
    use windows::Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::{
            EnumWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
            IsWindowVisible,
        },
    };
    let mut instance = DesktopInstance::new();
    let result = success(
        instance
            .command()
            .args(["notify", "Native notification check"])
            .output()
            .unwrap(),
    );
    instance.pid = Some(result["host_pid"].as_u64().unwrap() as u32);
    assert_eq!(result["ok"], true);
    assert!(result.get("id").is_none());
    let list = success(instance.command().arg("list").output().unwrap());
    assert_eq!(list["reminders"], serde_json::json!([]));
    assert_eq!(list["host_pid"], result["host_pid"]);
    unsafe extern "system" fn inspect(window: HWND, state: LPARAM) -> windows::core::BOOL {
        let state = unsafe { &mut *(state.0 as *mut (u32, Vec<String>)) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut pid));
            if pid == state.0 && IsWindowVisible(window).as_bool() {
                let mut title = [0; 256];
                let length = GetWindowTextW(window, &mut title);
                state
                    .1
                    .push(String::from_utf16_lossy(&title[..length as usize]));
            }
        }
        true.into()
    }
    let started = std::time::Instant::now();
    let mut state = (instance.pid.unwrap(), Vec::<String>::new());
    loop {
        state.1.clear();
        unsafe {
            EnumWindows(
                Some(inspect),
                LPARAM((&mut state as *mut (u32, Vec<String>)) as isize),
            )
            .unwrap();
        }
        if !state.1.is_empty() || started.elapsed().as_secs() >= 3 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(state.1, vec!["\u{421}\u{442}\u{43e}\u{43f}\u{43a}\u{430} \u{43d}\u{430}\u{43f}\u{43e}\u{43c}\u{438}\u{43d}\u{430}\u{43d}\u{438}\u{439}".to_string()]);
    let mut foreground_pid = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut foreground_pid));
    }
    assert_ne!(
        foreground_pid,
        instance.pid.unwrap(),
        "notification activated its process"
    );
    let invalid = instance.command().args(["notify", "   "]).output().unwrap();
    assert!(!invalid.status.success());
    assert!(!invalid.stderr.is_empty());
    assert!(invalid.stdout.is_empty());
    let extra = instance
        .command()
        .args(["notify", "valid", "extra"])
        .output()
        .unwrap();
    assert!(!extra.status.success());
    assert!(extra.stdout.is_empty());
}
