#![cfg(windows)]
use serde_json::{Value, json};
use std::{io::Write, process::Stdio};

#[path = "support/desktop.rs"]
mod desktop;
use desktop::DesktopInstance as Instance;

impl Instance {
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
    instance.stop().unwrap();
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

#[test]
fn immediate_notification_succeeds_while_an_unrelated_due_reminder_cannot_save() {
    let mut instance = Instance::new();
    instance.run("create",Some(&json!({"text":"Due schedule", "schedule":{"kind":"after_seconds","seconds":1},"sound":false})));
    std::fs::create_dir(instance.dir.path().join("reminders.pending")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1300));
    let output = instance
        .command()
        .args(["notify", "Independent notification"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], true);
}
