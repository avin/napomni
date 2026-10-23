#![cfg(windows)]
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

struct Instance {
    dir: tempfile::TempDir,
    pid: Option<u32>,
}
impl Instance {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("App with spaces");
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
        let mut command = Command::new(self.dir.path().join("App with spaces/napomni-cli.exe"));
        command
            .env("NAPOMNI_DATA_DIR", self.dir.path())
            .env_remove("NAPOMNI_APP_EXE");
        command
    }
    fn run(&mut self, command: &str, arguments: Option<&Value>) -> Value {
        let mut child = self.command();
        child.arg(command);
        if arguments.is_some() {
            child.arg("-").stdin(Stdio::piped());
        }
        let mut child = child
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(arguments) = arguments {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(arguments.to_string().as_bytes())
                .unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        if let Some(pid) = value["host_pid"].as_u64() {
            self.pid = Some(pid as u32);
        }
        value
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output();
        }
    }
}

#[test]
fn real_cli_creates_updates_pauses_resumes_undoes_and_deletes_the_same_record() {
    let mut instance = Instance::new();
    let created=instance.run("create",Some(&json!({"text":"До \"кавычек\"\nстрока", "schedule":{"kind":"after_seconds","seconds":3600}, "sound":false})));
    let id = created["reminder"]["id"].clone();
    assert_eq!(
        instance.run("list", None)["reminders"][0]["reminder"],
        created["reminder"]
    );
    let updated = instance.run("update", Some(&json!({"id":id,"patch":{"text":"New"}})));
    assert_eq!(updated["reminder"]["text"], "New");
    assert_eq!(updated["reminder"]["sound"], false);
    assert_eq!(
        instance.run("pause", Some(&json!({"id":id})))["reminder"]["paused"],
        true
    );
    assert_eq!(
        instance.run("resume", Some(&json!({"id":id})))["reminder"]["paused"],
        false
    );
    instance.run("undo", None);
    assert_eq!(
        instance.run("list", None)["reminders"][0]["reminder"]["paused"],
        true
    );
    assert_eq!(
        instance.run("delete", Some(&json!({"id":id})))["deleted_id"],
        id
    );
    assert_eq!(instance.run("list", None)["reminders"], json!([]));
}

#[test]
fn cli_exposes_machine_readable_argument_schemas_without_starting_app() {
    let mut instance = Instance::new();
    let schema = instance.run("schema", None);
    assert_eq!(
        schema["create"]["properties"]["schedule"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        9
    );
    assert_eq!(schema["update"]["properties"]["patch"]["minProperties"], 1);
    assert_eq!(schema["pause"]["required"], json!(["id"]));
    assert!(instance.pid.is_none());
}

#[test]
fn real_cli_reports_save_failure_keeps_undo_and_restores_committed_data_after_restart() {
    let mut instance = Instance::new();
    let created=instance.run("create",Some(&json!({"text":"Persisted", "schedule":{"kind":"interval","seconds":3600},"sound":false})));
    let id = created["reminder"]["id"].clone();
    let pending = instance.dir.path().join("reminders.pending");
    std::fs::create_dir(&pending).unwrap();
    let output = instance
        .command()
        .arg("update")
        .arg(json!({"id":id,"patch":{"text":"Not saved"}}).to_string())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
    assert_eq!(
        instance.run("list", None)["reminders"][0]["reminder"],
        created["reminder"]
    );
    std::fs::remove_dir(pending).unwrap();
    let pid = instance.pid.take().unwrap();
    use windows::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid).unwrap() };
    assert!(
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .unwrap()
            .status
            .success()
    );
    unsafe {
        assert_eq!(WaitForSingleObject(process, 5000), WAIT_OBJECT_0);
        let _ = CloseHandle(process);
    }
    assert_eq!(
        instance.run("list", None)["reminders"][0]["reminder"],
        created["reminder"]
    );
    instance.run("undo", None);
    assert_eq!(instance.run("list", None)["reminders"], json!([]));
}

#[test]
fn real_cli_rejects_bad_json_unknown_ids_and_unknown_fields_without_changing_undo() {
    let mut instance = Instance::new();
    let created = instance.run(
        "create",
        Some(&json!({"text":"Saved", "schedule":{"kind":"after_seconds","seconds":3600}})),
    );
    let id = created["reminder"]["id"].clone();
    for (command, args) in [
        ("update", "broken".to_owned()),
        ("update", "[]".to_owned()),
        (
            "update",
            json!({"id":999999,"patch":{"text":"No"}}).to_string(),
        ),
        ("pause", json!({"id":id,"paused":false}).to_string()),
        (
            "create",
            json!({"text":"No","schedule":{"kind":"interval","seconds":0}}).to_string(),
        ),
    ] {
        let output = instance.command().args([command, &args]).output().unwrap();
        assert!(!output.status.success());
        assert!(!output.stderr.is_empty());
    }
    assert_eq!(
        instance.run("list", None)["reminders"][0]["reminder"],
        created["reminder"]
    );
    instance.run("undo", None);
    assert_eq!(instance.run("list", None)["reminders"], json!([]));
}
