// In release builds on Windows, do not open a console window.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod cli;
mod colfilter;
mod config;
mod errcodes;
mod excerpt;
mod export;
mod fileassoc;
mod filter;
mod guides;
mod http;
mod httpstatus;
mod i18n;
mod icon;
mod logdef;
mod logset;
mod merge;
mod overview;
mod parser;
mod splash;
mod store;
mod tabexport;
mod timefmt;
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
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew, wgpu};
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

/// What the program opens with: log files from the command line (several are merged into one
/// view) with the view options (#26), or the state of the previous version after an update
/// (`--restore <file>`).
#[derive(Clone, Default)]
struct Startup {
    paths: Vec<std::path::PathBuf>,
    view: cli::ViewOptions,
    /// `--lang`: UI language for this run only.
    lang: Option<String>,
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
            Ok(Box::new(app::App::new(
                cc,
                startup.paths,
                startup.view,
                startup.lang,
                startup.restore,
            )))
        }),
    )
}

fn main() -> eframe::Result {
    let (cfg, _) = config::Config::load();
    i18n::set_language(&cfg.language);
    let startup = command_line();
    update::cleanup_old();

    // Start-up splash until the main window has drawn its first frame (#11), in the language
    // (or `--lang`) and theme of the settings.
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

/// Reads the command line (#26): log paths (e.g. the .log file association; several = one merged
/// view) with options, or `--restore <file>` from the previous version that has just installed
/// this one. `--help`, `--version` and invalid options show a message and end the program.
fn command_line() -> Startup {
    let fail = |e: cli::CliError| -> ! {
        let msg = e.message();
        cli::show_message(&t!("cli.title.error"), &msg, &msg, true);
        std::process::exit(2)
    };
    let opts = cli::parse(std::env::args_os().skip(1)).unwrap_or_else(|e| fail(e));
    let languages: Vec<String> = i18n::available().into_iter().map(|l| l.code).collect();
    // The language first, so the help and the messages are already in it.
    let lang = match opts.lang.as_deref().map(|l| cli::check_lang(l, &languages)) {
        Some(Ok(code)) => {
            i18n::set_language(&code);
            Some(code)
        }
        Some(Err(e)) if !opts.help && !opts.version => fail(e),
        _ => None,
    };
    if opts.help || opts.version {
        if opts.help {
            let langs = languages.join(", ");
            // Aligned with spaces on the console, with tabs in the message box.
            cli::show_message(
                &t!("cli.title.help"),
                &cli::help_text(&langs, false),
                &cli::help_text(&langs, true),
                false,
            );
        } else {
            let text = app::base_title();
            cli::show_message(&t!("cli.title.version"), &text, &text, false);
        }
        std::process::exit(0)
    }
    if let Some(file) = &opts.restore {
        return Startup {
            restore: update::SessionState::take(file),
            ..Default::default()
        };
    }
    if let Err(e) = cli::validate(&opts.view, &t!("format.date")) {
        fail(e)
    }
    Startup {
        paths: opts.files,
        view: opts.view,
        lang,
        restore: None,
    }
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
    let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
        run(renderer, startup.clone(), &created)
    }));
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
