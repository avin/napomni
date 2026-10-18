use anyhow::{Result, bail};
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{HWND, POINT},
    Graphics::{
        Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute},
        Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint},
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
    square_corners(window);
    if let Ok(handle) = hwnd(window) {
        unsafe {
            let _ = ShowWindow(handle, SW_SHOW);
        }
    }
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
    }))
}
