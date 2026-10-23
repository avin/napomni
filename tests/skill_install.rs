use napomni::skill_install::{default_root, install};
use serde_json::Value;

#[test]
fn settings_install_creates_one_skill_with_the_actual_cli_path() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("App with spaces");
    std::fs::create_dir(&app).unwrap();
    let cli = app.join("napomni-cli.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_napomni-cli"), &cli).unwrap();
    let root = temp.path().join("Chosen skills");
    let installed = install(&root, &cli).unwrap();
    assert_eq!(installed, root.join("napomni"));
    assert!(installed.join("SKILL.md").is_file());
    let resource: Value =
        serde_json::from_slice(&std::fs::read(installed.join("cli-path.json")).unwrap()).unwrap();
    assert_eq!(
        std::fs::canonicalize(resource["cli"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(cli).unwrap()
    );
    let output = std::process::Command::new(resource["cli"].as_str().unwrap())
        .arg("schema")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(schema["create"]["properties"]["schedule"].is_object());
    assert_eq!(
        default_root().unwrap(),
        std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".agents/skills")
    );
}

#[test]
fn choosing_another_root_keeps_the_old_copy_and_other_skills() {
    let temp = tempfile::tempdir().unwrap();
    let cli = temp.path().join("napomni-cli.exe");
    std::fs::write(&cli, b"location").unwrap();
    let first = temp.path().join("first root");
    let second = temp.path().join("second root");
    std::fs::create_dir_all(second.join("neighbor")).unwrap();
    std::fs::write(second.join("neighbor/SKILL.md"), "Existing unrelated skill").unwrap();
    let old = install(&first, &cli).unwrap();
    let old_skill = std::fs::read(old.join("SKILL.md")).unwrap();
    let installed = install(&second, &cli).unwrap();
    assert!(installed.join("SKILL.md").is_file());
    assert_eq!(std::fs::read(old.join("SKILL.md")).unwrap(), old_skill);
    assert_eq!(
        std::fs::read_to_string(second.join("neighbor/SKILL.md")).unwrap(),
        "Existing unrelated skill"
    );
    install(&second, &cli).unwrap();
    assert_eq!(std::fs::read(old.join("SKILL.md")).unwrap(), old_skill);
}

#[test]
fn settings_install_reports_bad_paths_and_required_file_write_failures() {
    let temp = tempfile::tempdir().unwrap();
    let cli = temp.path().join("napomni-cli.exe");
    std::fs::write(&cli, b"location").unwrap();
    let file_root = temp.path().join("file");
    std::fs::write(&file_root, "keep").unwrap();
    assert!(install(&file_root, &cli).is_err());
    assert_eq!(std::fs::read_to_string(file_root).unwrap(), "keep");
    assert!(install(std::path::Path::new(""), &cli).is_err());
    assert!(
        install(
            &temp.path().join("missing cli"),
            &temp.path().join("absent.exe")
        )
        .is_err()
    );
    for (root_name, blocked_file) in [
        ("blocked skill", "SKILL.md"),
        ("blocked cli resource", "cli-path.json"),
    ] {
        let root = temp.path().join(root_name);
        std::fs::create_dir_all(root.join("napomni").join(blocked_file)).unwrap();
        assert!(install(&root, &cli).is_err());
    }
}
