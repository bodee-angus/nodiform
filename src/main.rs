mod app;
mod diagnostics;
mod editor;
mod gpu;
mod model;
mod recording;
mod rules;
mod timeline;

fn main() -> eframe::Result {
    let mode = match diagnostics::LaunchMode::parse(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(mode) => mode,
        Err(error) => {
            eprintln!("{error}\nUsage: nodiform [--version | --smoke-test]");
            std::process::exit(2);
        }
    };
    let (smoke_test, smoke_probe) = match mode {
        diagnostics::LaunchMode::Version => {
            println!("Nodiform {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        diagnostics::LaunchMode::RuleWorker => std::process::exit(rules::worker_main()),
        diagnostics::LaunchMode::SmokeTest => {
            let (test, probe) = diagnostics::SmokeTest::start();
            (Some(test), Some(probe))
        }
        diagnostics::LaunchMode::Normal => (None, None),
    };
    let mut wgpu_setup = eframe::egui_wgpu::WgpuSetupCreateNew::default();
    wgpu_setup.instance_descriptor.backends = eframe::wgpu::Backends::VULKAN;
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: wgpu_setup.into(),
            ..Default::default()
        },
        viewport: eframe::egui::ViewportBuilder::default()
            .with_app_id("nodiform")
            .with_title("Nodiform · Emergent Graph Laboratory")
            .with_inner_size([1500.0, 940.0])
            .with_min_inner_size([1024.0, 700.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Nodiform",
        options,
        Box::new(move |context| Ok(Box::new(app::NodiformApp::new(context, smoke_probe)))),
    );
    if let Some(test) = smoke_test {
        let passed = test.finish(result.as_ref().err().map(ToString::to_string));
        std::process::exit(if passed { 0 } else { 1 });
    }
    result
}
