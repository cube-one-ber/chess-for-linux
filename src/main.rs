#[cfg(not(target_arch = "wasm32"))]
mod app;
#[cfg(not(target_arch = "wasm32"))]
mod automation;
mod document;
mod engine;
mod game;
#[cfg(not(target_arch = "wasm32"))]
mod legacy_engine;
#[cfg(not(target_arch = "wasm32"))]
mod network;
#[cfg(not(target_arch = "wasm32"))]
mod recording;
mod render;
#[cfg(not(target_arch = "wasm32"))]
mod speech;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|s| s == "--help" || s == "-h") {
        println!(
            "Chess for Linux — Rust / Vulkan\n\nUsage: chess-linux [FILE] [--fresh]\n       chess-linux --render-check [OUTPUT.png]\n       chess-linux --gui-smoke\n       chess-linux --analyze FEN [VARIANT]\n       chess-linux --command JSON [--socket PATH]\n\nOpen .chess-linux, Apple .chess or PGN games.\n--fresh skips session recovery. See README.linux.md for setup and controls."
        );
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "--command") {
        let request = args.get(1).ok_or("--command requires a JSON request")?;
        let path = args
            .iter()
            .position(|s| s == "--socket")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        println!("{}", automation::send(request, path)?);
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "--render-check") {
        return render_check(
            args.get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| "artifacts/vulkan-board.png".into()),
        );
    }
    if args.first().is_some_and(|s| s == "--analyze") {
        let fen = args.get(1).ok_or("--analyze requires a quoted FEN")?;
        let rules = args
            .get(2)
            .map(|s| game::Rules::parse(s))
            .transpose()?
            .unwrap_or_default();
        let board = game::Board::from_fen(rules, fen)?;
        let analysis = engine::analyze(
            board,
            1.0,
            8,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        println!("{analysis:?}");
        return Ok(());
    }
    let smoke = args.iter().any(|s| s == "--gui-smoke");
    let fresh = smoke || args.iter().any(|s| s == "--fresh");
    let path = args.iter().find(|s| !s.starts_with('-')).map(PathBuf::from);
    let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::default();
    setup.instance_descriptor.backends = wgpu::Backends::VULKAN;
    let icon =
        image::load_from_memory(include_bytes!("../Resources/Icons/Chess_128x128.png"))?.to_rgba8();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: setup.into(),
            ..Default::default()
        },
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Chess · Linux")
            .with_inner_size([1280.0, 850.0])
            .with_min_inner_size([720.0, 560.0])
            .with_icon(eframe::egui::IconData {
                rgba: icon.clone().into_raw(),
                width: icon.width(),
                height: icon.height(),
            }),
        ..Default::default()
    };
    eframe::run_native(
        "Chess",
        options,
        Box::new(move |cc| Ok(Box::new(app::ChessApp::new(cc, path, fresh, smoke)))),
    )?;
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
fn render_check(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..Default::default()
    });
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    let info = adapter.get_info();
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
    let mut renderer = render::BoardRenderer::new(device, queue, info.name.clone());
    let game = game::Game::new(game::Rules::Standard);
    renderer.render(&game, &render::View::default(), None, None, None, None);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    renderer.save_png(&path)?;
    println!(
        "Vulkan verified on {}. Rendered board: {}",
        info.name,
        path.display()
    );
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}
