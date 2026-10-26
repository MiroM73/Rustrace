// In release builds on Windows, do not open a console window.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod colfilter;
mod config;
mod errcodes;
mod excerpt;
mod export;
mod fileassoc;
mod http;
mod i18n;
mod icon;
mod logdef;
mod parser;
mod splash;
mod store;
mod update;

use std::cell::Cell;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use eframe::egui;

/// The wgpu fallback renderer: only Direct3D 12 when a (non-software) DX12 adapter exists. By
/// default wgpu loads the Vulkan, DX12 and OpenGL drivers and the graphics stack alone takes much
/// more memory (measured with an empty window: 516 MB private bytes by default, 325 MB with DX12
/// only). Without DX12, or when `WGPU_BACKEND` is set, the defaults are kept.
#[cfg(windows)]
fn prefer_dx12() -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{wgpu, WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    if std::env::var_os("WGPU_BACKEND").is_none() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            flags: wgpu::InstanceFlags::from_build_config().with_env(),
            backend_options: wgpu::BackendOptions::from_env_or_default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let dx12 = block_on(instance.enumerate_adapters(wgpu::Backends::DX12))
            .iter()
            .any(|a| a.get_info().device_type != wgpu::DeviceType::Cpu);
        if dx12 {
            setup.instance_descriptor.backends = wgpu::Backends::DX12;
        }
    }
    WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(setup),
        ..Default::default()
    }
}

/// Runs a future that completes without waiting (wgpu's native adapter enumeration).
#[cfg(windows)]
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

/// What the program opens with: a log file from the command line, or the state of the previous
/// version after an update (`--restore <file>`).
#[derive(Clone, Default)]
struct Startup {
    path: Option<String>,
    restore: Option<update::SessionState>,
}

/// Opens the window with the given renderer; `created` is set once the app exists.
fn run(renderer: eframe::Renderer, startup: Startup, created: &Cell<bool>) -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default().with_inner_size([1400.0, 850.0]);
    if let Some(w) = startup.restore.as_ref().and_then(|s| s.window.as_ref()) {
        if let Some(pos) = w.pos {
            viewport = viewport.with_position(pos);
        }
        if let Some(size) = w.size {
            viewport = viewport.with_inner_size(size);
        }
        viewport = viewport.with_maximized(w.maximized);
    }
    let options = eframe::NativeOptions {
        viewport: viewport
            .with_min_inner_size([700.0, 400.0])
            .with_title(app::base_title())
            .with_icon(Arc::new(egui::IconData {
                rgba: icon::rgba(64),
                width: 64,
                height: 64,
            }))
            .with_drag_and_drop(true),
        renderer,
        // The DX12 probe only when wgpu is really used (it loads the DX12 driver).
        #[cfg(windows)]
        wgpu_options: if renderer == eframe::Renderer::Wgpu {
            prefer_dx12()
        } else {
            Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "RusTrace",
        options,
        Box::new(move |cc| {
            created.set(true);
            Ok(Box::new(app::App::new(cc, startup.path, startup.restore)))
        }),
    )
}

fn main() -> eframe::Result {
    // Optional: path to a log file as the first argument (e.g. .log file association), or
    // `--restore <file>` from the previous version that has just installed this one.
    let mut args = std::env::args().skip(1);
    let startup = match args.next() {
        Some(a) if a == update::RESTORE_ARG => Startup {
            path: None,
            restore: args.next().and_then(|f| update::SessionState::take(std::path::Path::new(&f))),
        },
        path => Startup { path, restore: None },
    };
    update::cleanup_old();

    // Start-up splash until the main window has drawn its first frame (#11), in the language and
    // theme of the settings.
    let (cfg, _) = config::Config::load();
    i18n::set_language(&cfg.language);
    let dark = match cfg.theme.as_str() {
        "dark" => true,
        "light" => false,
        _ => splash::system_dark(),
    };
    splash::show(&app::base_title(), &t!("splash.starting"), dark);
    let res = start(startup);
    splash::close();
    res
}

/// Opens the main window with OpenGL, or with wgpu when OpenGL fails.
fn start(startup: Startup) -> eframe::Result {
    // OpenGL (glow) by default: with wgpu (DX12 as well as Vulkan) the AMD driver kept one CPU
    // core fully busy while the window was idle (issue #6), and OpenGL also needs less memory.
    // When OpenGL cannot start (e.g. a remote desktop or VM with only the software OpenGL 1.1),
    // wgpu is used instead. `RUSTRACE_RENDERER=wgpu` / `glow` forces one of them.
    let forced = std::env::var("RUSTRACE_RENDERER").ok();
    let renderer = match forced.as_deref() {
        Some("wgpu") => eframe::Renderer::Wgpu,
        _ => eframe::Renderer::Glow,
    };
    let fallback = renderer == eframe::Renderer::Glow && forced.is_none();
    if !fallback {
        return run(renderer, startup, &Cell::new(false));
    }
    // eframe's OpenGL start-up can also panic (e.g. no matching pixel format), so a panic before
    // the app exists falls back too; later panics and errors are passed on.
    let created = Cell::new(false);
    let res = std::panic::catch_unwind(AssertUnwindSafe(|| run(renderer, startup.clone(), &created)));
    match res {
        Ok(Err(e)) if !created.get() => {
            eprintln!("OpenGL renderer failed ({e}), using wgpu");
            run(eframe::Renderer::Wgpu, startup, &created)
        }
        Err(_) if !created.get() => {
            eprintln!("OpenGL renderer failed, using wgpu");
            run(eframe::Renderer::Wgpu, startup, &created)
        }
        Ok(res) => res,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
