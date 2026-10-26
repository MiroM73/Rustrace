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

use eframe::egui;

/// Graphics backend: only Direct3D 12 when a (non-software) DX12 adapter exists. By default wgpu
/// loads the Vulkan, DX12 and OpenGL drivers and the graphics stack alone takes much more memory
/// (measured with an empty window: 516 MB private bytes by default, 325 MB with DX12 only).
/// Without DX12, or when `WGPU_BACKEND` is set, the defaults are kept.
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

fn main() -> eframe::Result {
    // Optional: path to a log file as the first argument (e.g. .log file association).
    let path = std::env::args().nth(1);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 850.0])
            .with_min_inner_size([700.0, 400.0])
            .with_title(app::base_title())
            .with_drag_and_drop(true),
        #[cfg(windows)]
        wgpu_options: prefer_dx12(),
        ..Default::default()
    };

    eframe::run_native(
        "RusTrace",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, path)))),
    )
}
