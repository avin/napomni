//! The public settings action for installing the bundled offline agent skill.
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

pub fn default_root() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("USERPROFILE").context("Не удалось определить папку пользователя")?,
    )
    .join(".agents")
    .join("skills"))
}

pub fn install(root: &Path, cli: &Path) -> Result<PathBuf> {
    ensure!(!root.as_os_str().is_empty(), "Укажите папку скиллов");
    ensure!(cli.is_file(), "Не найден CLI: {}", cli.display());
    let cli = std::path::absolute(cli).context("Не удалось определить путь CLI")?;
    let destination = root.join("napomni");
    match std::fs::symlink_metadata(&destination) {
        Ok(_) => {
            let root = root.canonicalize()?;
            let resolved = destination.canonicalize()?;
            ensure!(
                resolved == root.join("napomni"),
                "Папка скилла не должна быть ссылкой: {}",
                destination.display()
            );
            ensure!(
                !cli.canonicalize()?.starts_with(&resolved),
                "CLI не должен находиться внутри папки устанавливаемого скилла"
            );
            std::fs::remove_dir_all(&resolved)
                .with_context(|| format!("Не удалось очистить папку {}", destination.display()))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("Не удалось проверить папку скилла"),
    }
    std::fs::create_dir_all(&destination)
        .with_context(|| format!("Не удалось создать папку {}", destination.display()))?;
    let resource = serde_json::to_vec_pretty(&serde_json::json!({"cli":cli}))?;
    std::fs::write(destination.join("cli-path.json"), resource)
        .context("Не удалось записать путь CLI")?;
    std::fs::create_dir_all(destination.join("scripts"))
        .context("Не удалось создать папку ресурсов скилла")?;
    std::fs::write(
        destination.join("scripts/notify.ps1"),
        include_str!("../assets/agent-skill/napomni/scripts/notify.ps1"),
    )
    .context("Не удалось записать помощник уведомлений")?;
    std::fs::write(
        destination.join("SKILL.md"),
        include_str!("../assets/agent-skill/napomni/SKILL.md"),
    )
    .context("Не удалось записать скилл")?;
    Ok(destination)
}
