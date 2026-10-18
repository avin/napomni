use anyhow::{Result, bail};
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{HWND, POINT},
    Graphics::Gdi::{
        CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
        MonitorFromPoint, SetWindowRgn,
    },
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SW_HIDE, SW_SHOW, SWP_NOACTIVATE,
            SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_NOACTIVATE,
        },
    },
};

pub struct WorkArea {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub scale: f32,
}

pub fn primary_work_area() -> Result<WorkArea> {
    // Monitor information is in physical pixels; GPUI layout uses logical pixels.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        GetMonitorInfoW(monitor, &mut info).ok()?;
        let (mut x, mut y) = (96, 96);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y)?;
        Ok(WorkArea {
            left: info.rcWork.left,
            top: info.rcWork.top,
            right: info.rcWork.right,
            bottom: info.rcWork.bottom,
            scale: x as f32 / 96.0,
        })
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
    if let Ok(handle) = hwnd(window) {
        unsafe {
            let _ = ShowWindow(handle, SW_SHOW);
        }
    }
}

pub fn place_toast(window: &Window, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
    let handle = hwnd(window)?;
    unsafe {
        let style = GetWindowLongPtrW(handle, GWL_EXSTYLE);
        SetWindowLongPtrW(handle, GWL_EXSTYLE, style | WS_EX_NOACTIVATE.0 as isize);
        // A native region also lets clicks in the rounded corners reach the window below.
        let radius = (12.0 * window.scale_factor()) as i32;
        let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2);
        if region.is_invalid() {
            bail!("Не удалось создать скругление тоста");
        }
        if SetWindowRgn(handle, Some(region), true) == 0 {
            let _ = DeleteObject(region.into());
            bail!("Не удалось применить скругление тоста");
        }
        // Windows owns the region after successful SetWindowRgn.
        SetWindowPos(
            handle,
            Some(HWND_TOPMOST),
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )?;
    }
    Ok(())
}
