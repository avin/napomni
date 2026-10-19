use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Corner {
    #[default]
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}
impl Corner {
    pub const ALL: [Self; 4] = [
        Self::TopRight,
        Self::BottomRight,
        Self::BottomLeft,
        Self::TopLeft,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::TopRight => "Справа вверху",
            Self::TopLeft => "Слева вверху",
            Self::BottomRight => "Справа внизу",
            Self::BottomLeft => "Слева внизу",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub monitor: Option<String>,
    pub corner: Corner,
    pub autostart: bool,
    #[serde(default = "default_snooze_minutes")]
    pub snooze_minutes: u32,
}
fn default_snooze_minutes() -> u32 {
    5
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            monitor: None,
            corner: Corner::default(),
            autostart: false,
            snooze_minutes: default_snooze_minutes(),
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct WorkArea {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub scale: f32,
}
#[derive(Debug, Clone)]
pub struct Monitor {
    pub id: String,
    pub primary: bool,
    pub area: WorkArea,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
pub trait DesktopPlatform {
    fn monitors(&self) -> Result<Vec<Monitor>>;
    fn set_autostart(&self, enabled: bool) -> Result<()>;
}
/// Public application settings boundary; platform effects and storage are external.
pub struct SettingsService<P> {
    path: PathBuf,
    settings: Settings,
    platform: P,
}
impl<P: DesktopPlatform> SettingsService<P> {
    pub fn open(path: PathBuf, platform: P) -> Result<Self> {
        let settings: Settings = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .context("Не удалось прочитать настройки размещения")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            (1..=525_600).contains(&settings.snooze_minutes),
            "Время отложения должно быть от 1 до 525600 минут"
        );
        Ok(Self {
            path,
            settings,
            platform,
        })
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    pub fn monitors(&self) -> Result<Vec<Monitor>> {
        self.platform.monitors()
    }
    pub fn save(&mut self, next: Settings) -> Result<()> {
        ensure!(
            (1..=525_600).contains(&next.snooze_minutes),
            "Время отложения должно быть от 1 до 525600 минут"
        );
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Prepare a complete file before touching the user's Windows setting.
        let pending = self.path.with_extension("pending");
        std::fs::write(&pending, serde_json::to_vec_pretty(&next)?)?;
        if let Err(error) = self.platform.set_autostart(next.autostart) {
            let _ = std::fs::remove_file(&pending);
            return Err(error);
        }
        if let Err(error) = std::fs::rename(&pending, &self.path) {
            let _ = self.platform.set_autostart(self.settings.autostart);
            let _ = std::fs::remove_file(&pending);
            return Err(error.into());
        }
        self.settings = next;
        Ok(())
    }
    pub fn placement(&self) -> Result<Placement> {
        let monitors = self.monitors()?;
        let selected = self
            .settings
            .monitor
            .as_ref()
            .and_then(|id| monitors.iter().find(|m| &m.id == id))
            .or_else(|| monitors.iter().find(|m| m.primary))
            .or_else(|| monitors.first())
            .context("Нет доступного монитора")?;
        Placement::new(selected.area, self.settings.corner)
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub viewport: Rect,
    pub scale: f32,
    corner: Corner,
}
impl Placement {
    fn new(area: WorkArea, corner: Corner) -> Result<Self> {
        ensure!(
            area.scale.is_finite() && area.scale > 0.,
            "Некорректный масштаб монитора"
        );
        let margin = (16. * area.scale).round() as i32;
        let width = (area.right - area.left - 2 * margin).min((368. * area.scale).round() as i32);
        let height = area.bottom - area.top - 2 * margin;
        ensure!(width > 0 && height > 0, "Рабочая область слишком мала");
        let x = match corner {
            Corner::TopRight | Corner::BottomRight => area.right - margin - width,
            _ => area.left + margin,
        };
        Ok(Self {
            viewport: Rect {
                x,
                y: area.top + margin,
                width,
                height,
            },
            scale: area.scale,
            corner,
        })
    }
    /// Position within the visible viewport; overflow belongs in a scrollable stack.
    pub fn card(&self, index: usize, logical_height: f32) -> Option<Rect> {
        if !logical_height.is_finite() || logical_height <= 0. {
            return None;
        }
        let height = ((logical_height * self.scale).round() as i32).min(self.viewport.height);
        let offset = i32::try_from(index)
            .ok()?
            .checked_mul(height + (12. * self.scale).round() as i32)?;
        if offset + height > self.viewport.height {
            return None;
        }
        let y = match self.corner {
            Corner::BottomRight | Corner::BottomLeft => {
                self.viewport.y + self.viewport.height - height - offset
            }
            _ => self.viewport.y + offset,
        };
        Some(Rect {
            x: self.viewport.x,
            y,
            width: self.viewport.width,
            height,
        })
    }
}
