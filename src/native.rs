use anyhow::{Result, bail};
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::{
        Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute},
        Gdi::{ClientToScreen, GetMonitorInfoW, MONITORINFO},
    },
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::{
            GWL_EXSTYLE, GetClientRect, GetWindowLongPtrW, GetWindowRect, HWND_TOPMOST, SW_HIDE,
            SW_SHOW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, ShowWindow,
            WS_EX_NOACTIVATE,
        },
    },
};

pub use napomni::placement::WorkArea;

pub struct WindowsDesktop;

impl napomni::placement::DesktopPlatform for WindowsDesktop {
    fn monitors(&self) -> Result<Vec<napomni::placement::Monitor>> {
        use windows::Win32::{
            Foundation::{LPARAM, RECT},
            Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR, MONITORINFOEXW},
        };
        unsafe extern "system" fn collect(
            monitor: HMONITOR,
            _: HDC,
            _: *mut RECT,
            data: LPARAM,
        ) -> windows::core::BOOL {
            let result = unsafe { &mut *(data.0 as *mut Vec<napomni::placement::Monitor>) };
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if unsafe { GetMonitorInfoW(monitor, &mut info as *mut _ as *mut MONITORINFO) }
                .as_bool()
            {
                let (mut x, mut y) = (96, 96);
                if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) }.is_ok() {
                    let work = info.monitorInfo.rcWork;
                    let end = info
                        .szDevice
                        .iter()
                        .position(|c| *c == 0)
                        .unwrap_or(info.szDevice.len());
                    result.push(napomni::placement::Monitor {
                        id: String::from_utf16_lossy(&info.szDevice[..end]),
                        primary: info.monitorInfo.dwFlags & 1 != 0,
                        area: WorkArea {
                            left: work.left,
                            top: work.top,
                            right: work.right,
                            bottom: work.bottom,
                            scale: x as f32 / 96.0,
                        },
                    });
                }
            }
            true.into()
        }
        let mut monitors = Vec::new();
        unsafe {
            EnumDisplayMonitors(
                None,
                None,
                Some(collect),
                LPARAM(&mut monitors as *mut _ as isize),
            )
            .ok()?;
        }
        anyhow::ensure!(
            !monitors.is_empty(),
            "Не удалось получить рабочие области мониторов"
        );
        Ok(monitors)
    }

    fn set_autostart(&self, enabled: bool) -> Result<()> {
        use windows::{
            Win32::{Foundation::ERROR_FILE_NOT_FOUND, System::Registry::*},
            core::{PCWSTR, w},
        };
        let mut key = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
            .ok()?;
            let result = if enabled {
                let command = format!("\"{}\"", std::env::current_exe()?.display());
                let bytes: Vec<u8> = command
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect();
                RegSetValueExW(key, w!("napomni"), None, REG_SZ, Some(&bytes))
            } else {
                let result = RegDeleteValueW(key, w!("napomni"));
                if result == ERROR_FILE_NOT_FOUND {
                    windows::Win32::Foundation::ERROR_SUCCESS
                } else {
                    result
                }
            };
            let _ = RegCloseKey(key);
            result.ok()?;
        }
        Ok(())
    }
}

fn hwnd(window: &Window) -> Result<HWND> {
    match HasWindowHandle::window_handle(window)
        .map_err(|error| anyhow::anyhow!("Не удалось получить HWND: {error}"))?
        .as_raw()
    {
        RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut _)),
        _ => bail!("Требуется Windows"),
    }
}

pub fn hide(window: &Window) {
    if let Ok(handle) = hwnd(window) {
        // This hides only the main window, leaving the tray and toasts alive.
        unsafe {
            let _ = ShowWindow(handle, SW_HIDE);
        }
    }
}

pub fn show(window: &Window) {
    square_corners(window);
    if let Ok(handle) = hwnd(window) {
        if let Err(error) = set_window_icons(handle) {
            eprintln!("Не удалось установить иконку окна: {error:#}");
        }
        unsafe {
            let _ = ShowWindow(handle, SW_SHOW);
        }
    }
}

fn set_window_icons(handle: HWND) -> Result<()> {
    use windows::{
        Win32::{
            Foundation::{LPARAM, WPARAM},
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
                WindowsAndMessaging::{
                    ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON,
                    SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
                },
            },
        },
        core::PCWSTR,
    };
    unsafe {
        let module = GetModuleHandleW(None)?;
        let dpi = GetDpiForWindow(handle);
        for (kind, width, height) in [
            (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
            (ICON_BIG, SM_CXICON, SM_CYICON),
        ] {
            let icon = LoadImageW(
                Some(module.into()),
                PCWSTR(1 as _),
                IMAGE_ICON,
                GetSystemMetricsForDpi(width, dpi),
                GetSystemMetricsForDpi(height, dpi),
                LR_SHARED,
            )?;
            // Shared resource handles live for the process lifetime; do not destroy them.
            SendMessageW(
                handle,
                WM_SETICON,
                Some(WPARAM(kind as usize)),
                Some(LPARAM(icon.0 as isize)),
            );
        }
    }
    Ok(())
}

pub fn square_corners(window: &Window) {
    if let Ok(handle) = hwnd(window) {
        // Windows 11 rounds top-level windows independently of GPUI styling.
        // Earlier Windows versions already use square corners and ignore this preference.
        unsafe {
            let _ = DwmSetWindowAttribute(
                handle,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &DWMWCP_DONOTROUND as *const _ as *const _,
                std::mem::size_of_val(&DWMWCP_DONOTROUND) as u32,
            );
        }
    }
}

/// GPUI retains non-client insets even for a popup without a title bar.
fn client_insets(handle: HWND) -> Result<RECT> {
    unsafe {
        let mut client = RECT::default();
        let mut outer = RECT::default();
        let mut origin = POINT::default();
        GetClientRect(handle, &mut client)?;
        GetWindowRect(handle, &mut outer)?;
        ClientToScreen(handle, &mut origin).ok()?;
        Ok(RECT {
            left: origin.x - outer.left,
            top: origin.y - outer.top,
            right: outer.right - origin.x - (client.right - client.left),
            bottom: outer.bottom - origin.y - (client.bottom - client.top),
        })
    }
}

pub fn place_toast(
    window: &Window,
    cx: &gpui::App,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<gpui::Task<Result<()>>> {
    square_corners(window);
    let handle = hwnd(window)?;
    // SetWindowPos sends WM_SIZE synchronously. Run it outside App/Window updates
    // so GPUI can enter its resize callback and update the layout before drawing.
    Ok(cx.foreground_executor().spawn(async move {
        unsafe {
            let style = GetWindowLongPtrW(handle, GWL_EXSTYLE);
            SetWindowLongPtrW(handle, GWL_EXSTYLE, style | WS_EX_NOACTIVATE.0 as isize);
            // First move to the target monitor so WM_DPICHANGED updates its insets.
            SetWindowPos(
                handle,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE,
            )?;
            let insets = client_insets(handle)?;
            // Placement describes the drawable client area, not the outer HWND.
            SetWindowPos(
                handle,
                Some(HWND_TOPMOST),
                x - insets.left,
                y - insets.top,
                width + insets.left + insets.right,
                height + insets.top + insets.bottom,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )?;
        }
        Ok(())
    }))
}

/// Restrict both painting and mouse input to the visible square card surfaces.
/// Alpha compositing by itself does not make a Windows popup click-through.
pub fn toast_region(
    window: &Window,
    cx: &gpui::App,
    rectangles: Vec<napomni::placement::Rect>,
) -> Result<gpui::Task<Result<()>>> {
    use windows::Win32::Graphics::Gdi::{
        CombineRgn, CreateRectRgn, DeleteObject, HGDIOBJ, RGN_OR, SetWindowRgn,
    };
    let handle = hwnd(window)?;
    // SetWindowRgn also sends window-position messages synchronously.
    Ok(cx.foreground_executor().spawn(async move {
        unsafe {
            // SetWindowRgn takes window coordinates; card rectangles use client coordinates.
            let insets = client_insets(handle)?;
            let region = CreateRectRgn(0, 0, 0, 0);
            anyhow::ensure!(!region.0.is_null(), "Не удалось создать область стопки");
            for rect in rectangles {
                let left = rect.x + insets.left;
                let top = rect.y + insets.top;
                let part = CreateRectRgn(left, top, left + rect.width, top + rect.height);
                if part.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(region.0));
                    bail!("Не удалось создать область карточки");
                }
                let combined = CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
                let _ = DeleteObject(HGDIOBJ(part.0));
                if combined.0 == 0 {
                    let _ = DeleteObject(HGDIOBJ(region.0));
                    bail!("Не удалось объединить области карточек");
                }
            }
            if SetWindowRgn(handle, Some(region), true) == 0 {
                let _ = DeleteObject(HGDIOBJ(region.0));
                bail!("Не удалось применить область стопки");
            }
            // Windows owns region after successful SetWindowRgn.
        }
        Ok(())
    }))
}

pub fn toast_allow_focus(window: &Window, allow: bool) {
    if let Ok(handle) = hwnd(window) {
        unsafe {
            let style = GetWindowLongPtrW(handle, GWL_EXSTYLE);
            let style = if allow {
                style & !(WS_EX_NOACTIVATE.0 as isize)
            } else {
                style | WS_EX_NOACTIVATE.0 as isize
            };
            SetWindowLongPtrW(handle, GWL_EXSTYLE, style);
        }
    }
}

pub fn cursor_position() -> Option<(i32, i32)> {
    use windows::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};
    let mut point = POINT::default();
    unsafe {
        GetCursorPos(&mut point).ok()?;
    }
    Some((point.x, point.y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use napomni::placement::DesktopPlatform;
    use windows::{
        Win32::{Foundation::ERROR_FILE_NOT_FOUND, System::Registry::*},
        core::w,
    };

    #[test]
    #[ignore = "writes the current user's Run value briefly, then restores it"]
    fn windows_autostart_roundtrip() -> Result<()> {
        struct Restore {
            key: HKEY,
            previous: Option<(REG_VALUE_TYPE, Vec<u8>)>,
        }
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    if let Some((kind, bytes)) = &self.previous {
                        let _ = RegSetValueExW(self.key, w!("napomni"), None, *kind, Some(bytes));
                    } else {
                        let _ = RegDeleteValueW(self.key, w!("napomni"));
                    }
                    let _ = RegCloseKey(self.key);
                }
            }
        }
        unsafe fn read(key: HKEY) -> Result<Option<(REG_VALUE_TYPE, Vec<u8>)>> {
            let mut kind = REG_VALUE_TYPE::default();
            let mut len = 0;
            let result = unsafe {
                RegQueryValueExW(
                    key,
                    w!("napomni"),
                    None,
                    Some(&mut kind),
                    None,
                    Some(&mut len),
                )
            };
            if result == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            result.ok()?;
            let mut bytes = vec![0; len as usize];
            unsafe {
                RegQueryValueExW(
                    key,
                    w!("napomni"),
                    None,
                    Some(&mut kind),
                    Some(bytes.as_mut_ptr()),
                    Some(&mut len),
                )
                .ok()?;
            }
            Ok(Some((kind, bytes)))
        }
        unsafe {
            let mut key = HKEY::default();
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
                None,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                &mut key,
            )
            .ok()?;
            let _restore = Restore {
                key,
                previous: read(key)?,
            };
            WindowsDesktop.set_autostart(true)?;
            let (_, bytes) = read(key)?.expect("Run value must exist after enabling");
            let actual: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            assert_eq!(
                String::from_utf16_lossy(&actual).trim_end_matches('\0'),
                format!("\"{}\"", std::env::current_exe()?.display())
            );
            WindowsDesktop.set_autostart(false)?;
            assert!(read(key)?.is_none());
            assert!(!WindowsDesktop.monitors()?.is_empty());
        }
        Ok(())
    }
}
