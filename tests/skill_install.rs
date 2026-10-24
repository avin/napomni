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
fn settings_install_reports_bad_paths() {
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
}

#[test]
fn reinstall_removes_old_contents_and_preserves_neighboring_skills() {
    let temp = tempfile::tempdir().unwrap();
    let cli = temp.path().join("napomni-cli.exe");
    std::fs::write(&cli, b"location").unwrap();
    let root = temp.path().join("skills");
    let destination = root.join("napomni");
    std::fs::create_dir_all(destination.join("old/nested")).unwrap();
    std::fs::write(destination.join("old/nested/file.txt"), "obsolete").unwrap();
    std::fs::write(destination.join("old.txt"), "obsolete").unwrap();
    std::fs::create_dir(destination.join("SKILL.md")).unwrap();
    std::fs::create_dir(destination.join("cli-path.json")).unwrap();
    std::fs::create_dir_all(root.join("neighbor")).unwrap();
    std::fs::write(root.join("neighbor/SKILL.md"), "keep").unwrap();

    install(&root, &cli).unwrap();

    assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 3);
    assert!(destination.join("SKILL.md").is_file());
    assert!(destination.join("cli-path.json").is_file());
    assert!(destination.join("scripts/notify.ps1").is_file());
    assert_eq!(
        std::fs::read_to_string(root.join("neighbor/SKILL.md")).unwrap(),
        "keep"
    );
}

#[test]
fn reinstall_preserves_contents_when_cli_is_inside_the_skill_folder() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("napomni");
    std::fs::create_dir(&destination).unwrap();
    let cli = destination.join("napomni-cli.exe");
    std::fs::write(&cli, b"keep").unwrap();

    assert!(install(temp.path(), &cli).is_err());
    assert_eq!(std::fs::read(cli).unwrap(), b"keep");
}
