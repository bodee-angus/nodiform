mod app;
mod editor;
mod gpu;
mod model;
mod recording;
mod rules;
mod timeline;

fn main() -> eframe::Result {
    if std::env::args().any(|argument| argument == "--rule-worker") {
        std::process::exit(rules::worker_main());
    }
    let mut wgpu_setup = eframe::egui_wgpu::WgpuSetupCreateNew::default();
    wgpu_setup.instance_descriptor.backends = eframe::wgpu::Backends::VULKAN;
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: wgpu_setup.into(),
            ..Default::default()
        },
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Nodiform · Emergent Graph Laboratory")
            .with_inner_size([1500.0, 940.0])
            .with_min_inner_size([1024.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Nodiform",
        options,
        Box::new(|context| Ok(Box::new(app::NodiformApp::new(context)))),
    )
}
