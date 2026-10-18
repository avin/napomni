use std::{cell::Cell, path::PathBuf};
use win_notifi_toast::placement::*;
struct Platform(Cell<bool>);
impl DesktopPlatform for Platform {
    fn monitors(&self) -> anyhow::Result<Vec<Monitor>> {
        Ok(vec![Monitor {
            id: "primary".into(),
            primary: true,
            area: WorkArea {
                left: 0,
                top: 40,
                right: 1920,
                bottom: 1080,
                scale: 1.25,
            },
        }])
    }
    fn set_autostart(&self, enabled: bool) -> anyhow::Result<()> {
        self.0.set(enabled);
        Ok(())
    }
}

struct Screens(WorkArea);
impl DesktopPlatform for Screens {
    fn monitors(&self) -> anyhow::Result<Vec<Monitor>> {
        Ok(vec![Monitor {
            id: "selected".into(),
            primary: true,
            area: self.0,
        }])
    }
    fn set_autostart(&self, _: bool) -> anyhow::Result<()> {
        Ok(())
    }
}

#[test]
fn stack_stays_in_taskbar_work_area_on_each_scale_and_corner() {
    let path = std::env::temp_dir().join(format!("reminders-geometry-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    for scale in [1., 1.25, 1.5] {
        for area in [
            WorkArea {
                left: -1920,
                top: 40,
                right: 0,
                bottom: 1080,
                scale,
            },
            WorkArea {
                left: -1880,
                top: 0,
                right: 0,
                bottom: 1080,
                scale,
            },
            WorkArea {
                left: -1920,
                top: 0,
                right: -40,
                bottom: 1080,
                scale,
            },
            WorkArea {
                left: -1920,
                top: 0,
                right: 0,
                bottom: 1040,
                scale,
            },
        ] {
            let mut app = SettingsService::open(path.clone(), Screens(area)).unwrap();
            for corner in Corner::ALL {
                app.save(Settings {
                    corner,
                    ..Settings::default()
                })
                .unwrap();
                let placement = app.placement().unwrap();
                assert!(placement.viewport.x >= area.left && placement.viewport.y >= area.top);
                assert!(placement.viewport.x + placement.viewport.width <= area.right);
                assert!(placement.viewport.y + placement.viewport.height <= area.bottom);
                let rect = placement.card(0, 156.).unwrap();
                assert!(rect.y >= area.top && rect.y + rect.height <= area.bottom);
                assert!(placement.card(100, 156.).is_none());
            }
        }
    }
    std::fs::remove_file(path).unwrap();
}
fn path() -> PathBuf {
    std::env::temp_dir().join(format!("reminders-placement-{}.json", std::process::id()))
}
#[test]
fn settings_survive_restart_and_autostart_is_applied() {
    let path = path();
    let _ = std::fs::remove_file(&path);
    let mut app = SettingsService::open(path.clone(), Platform(Cell::new(false))).unwrap();
    assert_eq!(app.settings().corner, Corner::TopRight);
    app.save(Settings {
        monitor: Some("primary".into()),
        corner: Corner::BottomLeft,
        autostart: true,
    })
    .unwrap();
    drop(app);
    let app = SettingsService::open(path.clone(), Platform(Cell::new(false))).unwrap();
    assert_eq!(app.settings().corner, Corner::BottomLeft);
    assert!(app.settings().autostart);
    assert_eq!(
        app.placement().unwrap().card(0, 156.).unwrap(),
        Rect {
            x: 20,
            y: 865,
            width: 460,
            height: 195
        }
    );
    std::fs::remove_file(path).unwrap();
}
