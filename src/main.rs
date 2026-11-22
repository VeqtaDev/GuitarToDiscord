#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod config;
mod dsp;
mod theme;
mod vst;

fn main() -> eframe::Result {
    env_logger::init();
    if std::env::args().any(|a| a == "--selftest") {
        selftest();
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Guitar To Discord")
            .with_inner_size([1080.0, 760.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native("Guitar To Discord", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

/// Headless check: run the engine with the saved/auto config for 3 s
/// and print the meters. `guitar-to-discord --selftest`
fn selftest() {
    use std::sync::Arc;
    let cfg = config::Config::load();
    let outs = audio::list_outputs(None);
    let ins = audio::list_inputs(None);
    println!("inputs:  {:?}", ins.iter().map(|d| &d.name).collect::<Vec<_>>());
    println!("outputs: {:?}", outs.iter().map(|d| &d.name).collect::<Vec<_>>());
    let output = cfg
        .output_device
        .clone()
        .or_else(|| audio::guess_output(&outs))
        .or_else(|| outs.first().map(|d| d.name.clone()))
        .expect("no output device");
    let guitar = cfg.guitar_device.clone().or_else(|| audio::guess_guitar(&ins));
    let mic = cfg.mic_device.clone().or_else(|| audio::guess_mic(&ins, guitar.as_deref()));
    let shared = Arc::new(audio::Shared::default());
    shared.guitar_gate_db.set(-96.0);
    shared.mic_gate_db.set(-96.0);
    let settings = audio::EngineSettings {
        guitar: guitar.map(|device| audio::InputSel { host: cfg.host.clone(), device, channel: cfg.guitar_channel }),
        mic: mic.map(|device| audio::InputSel { host: None, device, channel: cfg.mic_channel }),
        output,
        monitor: audio::guess_headphones(&outs),
    };
    shared.monitor_mode.store(audio::MonitorMode::Both as u8, std::sync::atomic::Ordering::Relaxed);
    shared.monitor_gain_db.set(-60.0); // inaudible during the test
    match audio::Engine::start(settings, shared.clone()) {
        Ok(e) => {
            println!("started: {}", e.summary);
            for _ in 0..6 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                println!(
                    "guitar {:.4}  mic {:.4}  out {:.4}  monitor {:.6}  underruns {}",
                    shared.guitar_level.get(),
                    shared.mic_level.get(),
                    shared.out_level.get(),
                    shared.monitor_level.get(),
                    shared.underruns.load(std::sync::atomic::Ordering::Relaxed)
                );
            }
        }
        Err(e) => println!("engine error: {e:#}"),
    }
}
