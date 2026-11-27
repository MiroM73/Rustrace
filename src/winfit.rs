//! Keeps the main window on the free part of the screen at start-up (#74, Windows).
//!
//! eframe only limits the initial size to the size of the monitor, so on a small or scaled display
//! the window reaches under the taskbar and the bottom of the program (and of its menus) cannot be
//! seen.

/// A rectangle in physical screen pixels, laid out like the Win32 `RECT`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Where a window at `window` has to be to lie inside `area`: it is moved, and made smaller only
/// when it is larger than the area. `None` when it is inside already.
pub fn fit(window: Rect, area: Rect) -> Option<Rect> {
    let width = (window.right - window.left).min(area.right - area.left);
    let height = (window.bottom - window.top).min(area.bottom - area.top);
    // Not `clamp`: it panics on the empty area of a failed system call.
    let left = window.left.min(area.right - width).max(area.left);
    let top = window.top.min(area.bottom - height).max(area.top);
    let fitted = Rect {
        left,
        top,
        right: left + width,
        bottom: top + height,
    };
    (fitted != window).then_some(fitted)
}

/// Moves / shrinks the window so that it is inside the work area (the screen without the taskbar)
/// of its monitor. A maximized or minimized window is left alone. To be called before the window
/// is shown.
#[cfg(windows)]
pub fn fit_to_work_area(window: &impl raw_window_handle::HasWindowHandle) {
    use raw_window_handle::RawWindowHandle;

    // As in `splash.rs`: the same functions must be declared with the same types.
    type Handle = isize;

    #[repr(C)]
    #[derive(Default)]
    struct MonitorInfo {
        cb_size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn IsZoomed(hwnd: Handle) -> i32;
        fn IsIconic(hwnd: Handle) -> i32;
        fn GetWindowRect(hwnd: Handle, rect: *mut Rect) -> i32;
        fn MonitorFromWindow(hwnd: Handle, flags: u32) -> Handle;
        fn GetMonitorInfoW(monitor: Handle, info: *mut MonitorInfo) -> i32;
        fn GetDpiForWindow(hwnd: Handle) -> u32;
        fn GetSystemMetricsForDpi(index: i32, dpi: u32) -> i32;
        fn SetWindowPos(
            hwnd: Handle,
            after: Handle,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
    }

    const MONITOR_DEFAULTTONEAREST: u32 = 2;
    const SM_CXSIZEFRAME: i32 = 32;
    const SM_CXPADDEDBORDER: i32 = 92;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd: Handle = handle.hwnd.get();

    // SAFETY: `hwnd` is the live window the handle was borrowed from; the structures passed by
    // pointer are `repr(C)` copies of RECT and MONITORINFO, the latter with its size set.
    unsafe {
        if IsZoomed(hwnd) != 0 || IsIconic(hwnd) != 0 {
            return;
        }
        let mut rect = Rect::default();
        let mut info = MonitorInfo {
            cb_size: size_of::<MonitorInfo>() as u32,
            ..Default::default()
        };
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if GetWindowRect(hwnd, &mut rect) == 0 || GetMonitorInfoW(monitor, &mut info) == 0 {
            return;
        }
        // The left, right and bottom resize borders of a window are invisible: they may stick out
        // of the work area (a window snapped to a side of the screen has them there).
        let dpi = GetDpiForWindow(hwnd).max(96);
        let border = GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi)
            + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        let area = Rect {
            left: info.work.left - border,
            top: info.work.top,
            right: info.work.right + border,
            bottom: info.work.bottom + border,
        };
        if let Some(r) = fit(rect, area) {
            SetWindowPos(
                hwnd,
                0,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, width: i32, height: i32) -> Rect {
        Rect {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }

    #[test]
    fn window_inside_the_area_stays() {
        let area = rect(0, 0, 1920, 1032);
        assert_eq!(fit(rect(100, 50, 1400, 900), area), None);
        assert_eq!(fit(area, area), None);
    }

    #[test]
    fn window_under_the_taskbar_moves_up() {
        let area = rect(0, 0, 1920, 1032);
        assert_eq!(
            fit(rect(200, 300, 1400, 900), area),
            Some(rect(200, 132, 1400, 900))
        );
    }

    #[test]
    fn too_large_window_is_shrunk() {
        // 1400 × 850 logical pixels at 150 % on a 1920 × 1080 screen.
        let area = rect(0, 0, 1920, 1008);
        assert_eq!(
            fit(rect(104, 104, 2116, 1322), area),
            Some(rect(0, 0, 1920, 1008))
        );
        // Only too tall: the width and the horizontal place stay.
        assert_eq!(
            fit(rect(104, 104, 1766, 1110), area),
            Some(rect(104, 0, 1766, 1008))
        );
    }

    #[test]
    fn monitor_with_negative_coordinates_and_taskbar_on_top() {
        // A monitor left of the primary one, its taskbar at the top.
        let area = rect(-1280, 40, 1280, 984);
        assert_eq!(
            fit(rect(-1400, 0, 800, 600), area),
            Some(rect(-1280, 40, 800, 600))
        );
        assert_eq!(
            fit(rect(-300, 700, 800, 600), area),
            Some(rect(-800, 424, 800, 600))
        );
    }
}
