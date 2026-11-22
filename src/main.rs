#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod config;
mod dsp;
mod theme;
mod vst;
mod vst_host;

fn main() -> eframe::Result {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--vsttest-one") {
        vst_test_one(std::path::Path::new(&args[i + 1]));
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--editortest") {
        editor_test(std::path::Path::new(&args[i + 1]));
        return Ok(());
    }
    if args.iter().any(|a| a == "--vsttest") {
        vst_test_all();
        return Ok(());
    }
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
        chain: None,
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

/// Load every scanned VST3 in a child process and run a test signal through it.
fn vst_test_all() {
    let plugins = vst::scan(&vst::default_dirs());
    let exe = std::env::current_exe().unwrap();
    for p in plugins {
        let out = std::process::Command::new(&exe).arg("--vsttest-one").arg(&p.path).output();
        let line = match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Ok(o) => format!("CRASH (exit {:?}) {}", o.status.code(), String::from_utf8_lossy(&o.stdout).trim()),
            Err(e) => format!("spawn error {e}"),
        };
        println!("{:<40} {}", p.name, line);
    }
}

fn vst_test_one(path: &std::path::Path) {
    use std::io::Write;
    let t0 = std::time::Instant::now();
    let mut host = vst_host::PluginHost::default();
    let entry = config::ChainEntry::new(path.to_path_buf());
    host.sync(std::slice::from_ref(&entry));
    if let Some(Err(e)) = host.instances.first() {
        println!("LOAD FAIL: {e}");
        return;
    }
    let _ = std::io::stdout().flush();
    host.start(48_000.0);
    let rate = 48_000.0f32;
    let secs: usize = std::env::var("GTD_TEST_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let mut buf: Vec<f32> = (0..24_000 * secs).map(|i| 0.3 * (i as f32 * 110.0 * std::f32::consts::TAU / rate).sin()).collect();
    let rms_in = (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt();
    {
        let mut slot = host.slot.lock().unwrap();
        if let Some(c) = slot.as_mut() {
            for chunk in buf.chunks_mut(480) {
                c.process(chunk);
            }
        }
    }
    let tail = &buf[buf.len() / 2..];
    let rms_out = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
    let mut entries = vec![entry];
    host.save_states(&mut entries);
    let st = entries[0].state.as_ref().map(|s| s.len()).unwrap_or(0);
    let inst = host.instances[0].as_ref().unwrap();
    println!(
        "OK  {}->{} ch  rms in {:.3} out {:.3}  state {}B  editor {}  {} ms",
        inst.in_ch, inst.out_ch, rms_in, rms_out, st, inst.has_editor(), t0.elapsed().as_millis()
    );
    drop(host);
}

/// Open one plugin's editor and pump Windows messages for a few seconds.
fn editor_test(path: &std::path::Path) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};
    let mut host = vst_host::PluginHost::default();
    host.sync(&[config::ChainEntry::new(path.to_path_buf())]);
    host.start(48_000.0);
    let inst = match host.instances.get_mut(0) {
        Some(Ok(i)) => i,
        Some(Err(e)) => return println!("LOAD FAIL {e}"),
        None => return,
    };
    match inst.open_editor() {
        Ok(()) => println!("editor opened"),
        Err(e) => return println!("EDITOR FAIL {e:#}"),
    }
    let t0 = std::time::Instant::now();
    while t0.elapsed() < std::time::Duration::from_secs(8) {
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        host.poll_editors();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    println!("closing");
    drop(host);
    println!("closed cleanly");
}
