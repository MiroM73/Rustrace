// In release builds on Windows, do not open a console window.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod colfilter;
mod config;
mod errcodes;
mod excerpt;
mod export;
mod fileassoc;
mod i18n;
mod logdef;
mod parser;
mod store;

use std::cell::Cell;
use std::panic::AssertUnwindSafe;

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

/// Opens the window with the given renderer; `created` is set once the app exists.
fn run(renderer: eframe::Renderer, path: Option<String>, created: &Cell<bool>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 850.0])
            .with_min_inner_size([700.0, 400.0])
            .with_title(app::base_title())
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
            Ok(Box::new(app::App::new(cc, path)))
        }),
    )
}

fn main() -> eframe::Result {
    // Optional: path to a log file as the first argument (e.g. .log file association).
    let path = std::env::args().nth(1);

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
        return run(renderer, path, &Cell::new(false));
    }
    // eframe's OpenGL start-up can also panic (e.g. no matching pixel format), so a panic before
    // the app exists falls back too; later panics and errors are passed on.
    let created = Cell::new(false);
    let res = std::panic::catch_unwind(AssertUnwindSafe(|| run(renderer, path.clone(), &created)));
    match res {
        Ok(Err(e)) if !created.get() => {
            eprintln!("OpenGL renderer failed ({e}), using wgpu");
            run(eframe::Renderer::Wgpu, path, &created)
        }
        Err(_) if !created.get() => {
            eprintln!("OpenGL renderer failed, using wgpu");
            run(eframe::Renderer::Wgpu, path, &created)
        }
        Ok(res) => res,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
