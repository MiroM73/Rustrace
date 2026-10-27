//! Start-up splash (issue #11): a small window with the icon, the program name and a rotating
//! ring of dots, shown right after the start until the main window has drawn its first frame
//! (creating the OpenGL / wgpu context can take a while, especially on a cold start).
//!
//! Windows only (raw user32 / gdi32 FFI, own thread with its own message loop); elsewhere the
//! functions do nothing. The whole window is painted into a 32-bit DIB (anti-aliased dots, the
//! icon from `icon::rgba`) and the texts are drawn on it with GDI.

#[cfg(not(windows))]
pub fn show(_title: &str, _text: &str, _dark: bool) {}

#[cfg(not(windows))]
pub fn close() {}

#[cfg(not(windows))]
pub fn system_dark() -> bool {
    false
}

#[cfg(windows)]
pub use imp::{close, show, system_dark};

#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)]
mod imp {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

    type Handle = isize;
    type WndProc = unsafe extern "system" fn(Handle, u32, usize, isize) -> isize;

    #[repr(C)]
    struct WNDCLASSEXW {
        cb_size: u32,
        style: u32,
        wnd_proc: WndProc,
        cls_extra: i32,
        wnd_extra: i32,
        instance: Handle,
        icon: Handle,
        cursor: Handle,
        background: Handle,
        menu_name: *const u16,
        class_name: *const u16,
        icon_sm: Handle,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct POINT {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct RECT {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct MSG {
        hwnd: Handle,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt: POINT,
        private: u32,
    }

    #[repr(C)]
    struct PAINTSTRUCT {
        hdc: Handle,
        erase: i32,
        paint: RECT,
        restore: i32,
        inc_update: i32,
        reserved: [u8; 32],
    }

    #[repr(C)]
    #[derive(Default)]
    struct MONITORINFO {
        cb_size: u32,
        monitor: RECT,
        work: RECT,
        flags: u32,
    }

    #[repr(C)]
    struct BITMAPINFO {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        x_ppm: i32,
        y_ppm: i32,
        clr_used: u32,
        clr_important: u32,
        colors: [u32; 1],
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterClassExW(wc: *const WNDCLASSEXW) -> u16;
        fn CreateWindowExW(
            ex_style: u32,
            class: *const u16,
            title: *const u16,
            style: u32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            parent: Handle,
            menu: Handle,
            instance: Handle,
            param: *mut c_void,
        ) -> Handle;
        fn DefWindowProcW(hwnd: Handle, msg: u32, wparam: usize, lparam: isize) -> isize;
        fn GetMessageW(msg: *mut MSG, hwnd: Handle, min: u32, max: u32) -> i32;
        fn TranslateMessage(msg: *const MSG) -> i32;
        fn DispatchMessageW(msg: *const MSG) -> isize;
        fn PostMessageW(hwnd: Handle, msg: u32, wparam: usize, lparam: isize) -> i32;
        fn DestroyWindow(hwnd: Handle) -> i32;
        fn PostQuitMessage(code: i32);
        fn SetTimer(hwnd: Handle, id: usize, ms: u32, func: *const c_void) -> usize;
        fn InvalidateRect(hwnd: Handle, rect: *const RECT, erase: i32) -> i32;
        fn BeginPaint(hwnd: Handle, ps: *mut PAINTSTRUCT) -> Handle;
        fn EndPaint(hwnd: Handle, ps: *const PAINTSTRUCT) -> i32;
        fn GetClientRect(hwnd: Handle, rect: *mut RECT) -> i32;
        fn GetCursorPos(pt: *mut POINT) -> i32;
        fn MonitorFromPoint(pt: POINT, flags: u32) -> Handle;
        fn GetMonitorInfoW(monitor: Handle, info: *mut MONITORINFO) -> i32;
        fn SetThreadDpiAwarenessContext(ctx: isize) -> isize;
        fn GetDpiForWindow(hwnd: Handle) -> u32;
        fn SetWindowPos(
            hwnd: Handle,
            after: Handle,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            flags: u32,
        ) -> i32;
        fn ShowWindow(hwnd: Handle, cmd: i32) -> i32;
        fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
        fn DrawTextW(hdc: Handle, text: *const u16, len: i32, rect: *mut RECT, format: u32) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(hdc: Handle) -> Handle;
        fn CreateDIBSection(
            hdc: Handle,
            info: *const BITMAPINFO,
            usage: u32,
            bits: *mut *mut c_void,
            section: Handle,
            offset: u32,
        ) -> Handle;
        fn SelectObject(hdc: Handle, obj: Handle) -> Handle;
        fn DeleteObject(obj: Handle) -> i32;
        fn DeleteDC(hdc: Handle) -> i32;
        fn BitBlt(
            dst: Handle,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            src: Handle,
            sx: i32,
            sy: i32,
            rop: u32,
        ) -> i32;
        fn SetBkMode(hdc: Handle, mode: i32) -> i32;
        fn SetTextColor(hdc: Handle, color: u32) -> u32;
        fn GdiFlush() -> i32;
        #[allow(clippy::too_many_arguments)]
        fn CreateFontW(
            height: i32,
            width: i32,
            escapement: i32,
            orientation: i32,
            weight: i32,
            italic: u32,
            underline: u32,
            strike_out: u32,
            charset: u32,
            out_precision: u32,
            clip_precision: u32,
            quality: u32,
            pitch_and_family: u32,
            face: *const u16,
        ) -> Handle;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> Handle;
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            key: Handle,
            sub_key: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> i32;
    }

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(hwnd: Handle, attr: u32, value: *const c_void, size: u32) -> i32;
    }

    const WM_DESTROY: u32 = 0x0002;
    const WM_PAINT: u32 = 0x000F;
    const WM_CLOSE: u32 = 0x0010;
    const WM_ERASEBKGND: u32 = 0x0014;
    const WM_TIMER: u32 = 0x0113;
    const WS_POPUP: u32 = 0x8000_0000;
    const WS_EX_TOPMOST: u32 = 0x0000_0008;
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
    const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
    const CS_DROPSHADOW: u32 = 0x0002_0000;
    const SW_SHOWNOACTIVATE: i32 = 4;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const MONITOR_DEFAULTTOPRIMARY: u32 = 1;
    const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
    const IDC_APPSTARTING: usize = 32650;
    const DT_SINGLELINE: u32 = 0x20;
    const DT_VCENTER: u32 = 0x4;
    const DT_END_ELLIPSIS: u32 = 0x8000;
    const SRCCOPY: u32 = 0x00CC_0020;
    const TRANSPARENT: i32 = 1;
    const CLEARTYPE_QUALITY: u32 = 5;
    const HKEY_CURRENT_USER: Handle = 0x8000_0001u32 as i32 as isize;
    const RRF_RT_REG_DWORD: u32 = 0x10;
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: u32 = 2;

    /// Size of the window in logical pixels (96 DPI).
    const WIDTH: f32 = 400.0;
    const HEIGHT: f32 = 120.0;
    const DOTS: usize = 8;
    const ACCENT: [f32; 3] = [43.0, 123.0, 214.0];

    static HWND: AtomicIsize = AtomicIsize::new(0);
    static CLOSED: AtomicBool = AtomicBool::new(false);

    /// What the window procedure draws.
    struct Painter {
        title: Vec<u16>,
        text: Vec<u16>,
        dark: bool,
        scale: f32,
        icon: Vec<u8>,
        icon_size: u32,
        frame: usize,
    }

    thread_local! {
        static PAINTER: RefCell<Option<Painter>> = const { RefCell::new(None) };
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Windows apps use the dark theme (Settings → Personalization → Colors).
    pub fn system_dark() -> bool {
        let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        let value = wide("AppsUseLightTheme");
        let mut data = 1u32;
        let mut size = 4u32;
        // SAFETY: valid NUL-terminated strings, `data` / `size` point to a DWORD buffer.
        let res = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&raw mut data).cast(),
                &mut size,
            )
        };
        res == 0 && data == 0
    }

    /// Shows the splash in its own thread (returns immediately).
    pub fn show(title: &str, text: &str, dark: bool) {
        let (title, text) = (wide(title), wide(text));
        let _ = std::thread::Builder::new()
            .name("rustrace-splash".into())
            // SAFETY: the thread owns the window and runs its message loop.
            .spawn(move || unsafe { run(title, text, dark) });
    }

    /// Closes the splash (also when it has not appeared yet; then it never does).
    pub fn close() {
        CLOSED.store(true, Ordering::SeqCst);
        let hwnd = HWND.load(Ordering::SeqCst);
        if hwnd != 0 {
            // SAFETY: posting to a window that no longer exists only fails.
            unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
        }
    }

    unsafe fn run(title: Vec<u16>, text: Vec<u16>, dark: bool) {
        unsafe {
            // Sharp on high-DPI screens: the window is sized and painted in physical pixels.
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(std::ptr::null());
            let class = wide("RusTraceSplash");
            let wc = WNDCLASSEXW {
                cb_size: size_of::<WNDCLASSEXW>() as u32,
                style: CS_DROPSHADOW,
                wnd_proc,
                cls_extra: 0,
                wnd_extra: 0,
                instance,
                icon: 0,
                cursor: LoadCursorW(0, IDC_APPSTARTING as *const u16),
                background: 0,
                menu_name: std::ptr::null(),
                class_name: class.as_ptr(),
                icon_sm: 0,
            };
            RegisterClassExW(&wc);

            // On the monitor with the mouse pointer (where the program was most likely started).
            let mut pt = POINT::default();
            GetCursorPos(&mut pt);
            let mut mi = MONITORINFO {
                cb_size: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY), &mut mi);
            let (cx, cy) = (
                (mi.work.left + mi.work.right) / 2,
                (mi.work.top + mi.work.bottom) / 2,
            );
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                cx,
                cy,
                1,
                1,
                0,
                0,
                instance,
                std::ptr::null_mut(),
            );
            if hwnd == 0 {
                return;
            }
            let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            let (w, h) = ((WIDTH * scale) as i32, (HEIGHT * scale) as i32);
            SetWindowPos(
                hwnd,
                0,
                cx - w / 2,
                cy - h / 2,
                w,
                h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let round = DWMWCP_ROUND;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const round).cast(),
                4,
            );
            let icon_size = (64.0 * scale).round() as u32;
            PAINTER.with_borrow_mut(|p| {
                *p = Some(Painter {
                    title,
                    text,
                    dark,
                    scale,
                    icon: crate::icon::rgba(icon_size),
                    icon_size,
                    frame: 0,
                })
            });
            HWND.store(hwnd, Ordering::SeqCst);
            if CLOSED.load(Ordering::SeqCst) {
                DestroyWindow(hwnd);
            } else {
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                SetTimer(hwnd, 1, 90, std::ptr::null());
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            HWND.store(0, Ordering::SeqCst);
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: Handle,
        msg: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        unsafe {
            match msg {
                WM_TIMER => {
                    PAINTER.with_borrow_mut(|p| {
                        if let Some(p) = p {
                            p.frame = p.frame.wrapping_add(1);
                        }
                    });
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                    0
                }
                WM_ERASEBKGND => 1,
                WM_PAINT => {
                    let mut ps: PAINTSTRUCT = std::mem::zeroed();
                    let hdc = BeginPaint(hwnd, &mut ps);
                    let mut rc = RECT::default();
                    GetClientRect(hwnd, &mut rc);
                    PAINTER.with_borrow(|p| {
                        if let Some(p) = p {
                            paint(p, hdc, rc.right, rc.bottom);
                        }
                    });
                    EndPaint(hwnd, &ps);
                    0
                }
                WM_CLOSE => {
                    DestroyWindow(hwnd);
                    0
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }

    /// Paints the whole window off-screen and copies it in one go (no flicker).
    unsafe fn paint(p: &Painter, hdc: Handle, w: i32, h: i32) {
        if w <= 0 || h <= 0 {
            return;
        }
        unsafe {
            let info = BITMAPINFO {
                size: 40,
                width: w,
                // Negative height: rows top to bottom.
                height: -h,
                planes: 1,
                bit_count: 32,
                compression: 0,
                size_image: 0,
                x_ppm: 0,
                y_ppm: 0,
                clr_used: 0,
                clr_important: 0,
                colors: [0],
            };
            let mem = CreateCompatibleDC(hdc);
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(hdc, &info, 0, &mut bits, 0, 0);
            if bmp == 0 || bits.is_null() {
                DeleteDC(mem);
                return;
            }
            let old_bmp = SelectObject(mem, bmp);
            let px = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (w * h * 4) as usize);
            draw_pixels(p, px, w, h);
            GdiFlush();

            let (fg, sub) = if p.dark {
                (0x00F0_F0F0, 0x00B0_B0B0)
            } else {
                (0x0020_2020, 0x0060_6060)
            };
            let s = p.scale;
            let face = wide("Segoe UI");
            SetBkMode(mem, TRANSPARENT);
            let x0 = (108.0 * s) as i32;
            let right = w - (80.0 * s) as i32;
            for (text, size, weight, color, top, bottom) in [
                (&p.title, 24.0, 600, fg, 28.0, 62.0),
                (&p.text, 16.0, 400, sub, 62.0, 90.0),
            ] {
                let font = CreateFontW(
                    -((size * s) as i32),
                    0,
                    0,
                    0,
                    weight,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    CLEARTYPE_QUALITY,
                    0,
                    face.as_ptr(),
                );
                let old_font = SelectObject(mem, font);
                SetTextColor(mem, color);
                let mut rc = RECT {
                    left: x0,
                    top: (top * s) as i32,
                    right,
                    bottom: (bottom * s) as i32,
                };
                DrawTextW(
                    mem,
                    text.as_ptr(),
                    -1,
                    &mut rc,
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                );
                SelectObject(mem, old_font);
                DeleteObject(font);
            }

            BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old_bmp);
            DeleteObject(bmp);
            DeleteDC(mem);
        }
    }

    /// Background, border, icon and the ring of dots into BGRA pixels.
    fn draw_pixels(p: &Painter, px: &mut [u8], w: i32, h: i32) {
        let bg: [f32; 3] = if p.dark {
            [32.0, 32.0, 32.0]
        } else {
            [250.0, 250.0, 250.0]
        };
        let border: [f32; 3] = if p.dark {
            [70.0, 70.0, 70.0]
        } else {
            [200.0, 200.0, 200.0]
        };
        let put = |px: &mut [u8], x: i32, y: i32, c: [f32; 3], a: f32| {
            if x < 0 || y < 0 || x >= w || y >= h || a <= 0.0 {
                return;
            }
            let i = ((y * w + x) * 4) as usize;
            for k in 0..3 {
                // BGRA: color k (RGB) goes to byte 2 - k.
                let old = px[i + 2 - k] as f32;
                px[i + 2 - k] = (old + (c[k] - old) * a.min(1.0)).round() as u8;
            }
            px[i + 3] = 255;
        };
        for y in 0..h {
            for x in 0..w {
                let edge = x == 0 || y == 0 || x == w - 1 || y == h - 1;
                let c = if edge { border } else { bg };
                let i = ((y * w + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[c[2] as u8, c[1] as u8, c[0] as u8, 255]);
            }
        }

        let s = p.scale;
        let size = p.icon_size as i32;
        let (ix, iy) = ((24.0 * s) as i32, (h - size) / 2);
        for y in 0..size {
            for x in 0..size {
                let i = ((y * size + x) * 4) as usize;
                let c = &p.icon[i..i + 4];
                let rgb = [c[0] as f32, c[1] as f32, c[2] as f32];
                put(px, ix + x, iy + y, rgb, c[3] as f32 / 255.0);
            }
        }

        // Ring of dots; the "head" is fully colored, the ones behind it fade out.
        let (cx, cy) = (w as f32 - 44.0 * s, h as f32 / 2.0);
        let (ring, r) = (14.0 * s, 3.4 * s);
        for i in 0..DOTS {
            let angle =
                i as f32 / DOTS as f32 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            let (dx, dy) = (cx + ring * angle.cos(), cy + ring * angle.sin());
            let behind = (p.frame + DOTS - i) % DOTS;
            let strength = [1.0, 0.75, 0.55, 0.4, 0.3, 0.22, 0.18, 0.15][behind];
            let reach = r.ceil() as i32 + 1;
            for y in dy as i32 - reach..=dy as i32 + reach {
                for x in dx as i32 - reach..=dx as i32 + reach {
                    let d = ((x as f32 + 0.5 - dx).powi(2) + (y as f32 + 0.5 - dy).powi(2)).sqrt();
                    let cover = (r + 0.5 - d).clamp(0.0, 1.0);
                    put(px, x, y, ACCENT, cover * strength);
                }
            }
        }
    }
}
