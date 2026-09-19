mod app;
mod births;
mod diagnostics;
mod editor;
mod experiment;
mod gpu;
mod inputs;
mod model;
mod recording;
mod rules;
mod theme;
mod timeline;

#[cfg(test)]
mod example_tests;

#[cfg(test)]
mod toroidal_tests;

#[cfg(test)]
mod grid_tests;

#[cfg(test)]
mod ring_lattice_tests;

#[cfg(test)]
mod colour_a_tests;

#[cfg(test)]
mod colour_b_tests;

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
    let default_descriptor = wgpu_setup.device_descriptor.clone();
    wgpu_setup.device_descriptor = std::sync::Arc::new(move |adapter| {
        let mut descriptor = default_descriptor(adapter);
        let supported = adapter.limits();
        descriptor.required_limits.max_buffer_size = supported.max_buffer_size;
        descriptor.required_limits.max_storage_buffer_binding_size =
            supported.max_storage_buffer_binding_size;
        descriptor
            .required_limits
            .max_compute_workgroups_per_dimension = supported.max_compute_workgroups_per_dimension;
        descriptor.required_limits.max_texture_dimension_2d = supported.max_texture_dimension_2d;
        descriptor
    });
    let window_size = if smoke_test.is_some()
        && std::env::var("NODIFORM_SMOKE_HIDPI").as_deref() == Ok("1")
    {
        [3000.0, 1880.0]
    } else if smoke_test.is_some() && std::env::var("NODIFORM_SMOKE_COMPACT").as_deref() == Ok("1")
    {
        [1024.0, 700.0]
    } else {
        [1500.0, 940.0]
    };
    let options = eframe::NativeOptions {
        persist_window: smoke_test.is_none(),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: wgpu_setup.into(),
            ..Default::default()
        },
        viewport: eframe::egui::ViewportBuilder::default()
            .with_app_id("nodiform")
            .with_title("Nodiform · Emergent Graph Laboratory")
            .with_inner_size(window_size)
            .with_min_inner_size([1024.0, 700.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Nodiform",
        options,
        Box::new(move |context| Ok(Box::new(app::NodiformApp::new(context, smoke_probe)?))),
    );
    if let Some(test) = smoke_test {
        let passed = test.finish(result.as_ref().err().map(ToString::to_string));
        std::process::exit(if passed { 0 } else { 1 });
    }
    result
}
