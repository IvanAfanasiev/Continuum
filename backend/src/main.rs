#[cfg(feature = "desktop-audio")]
fn main() {
    use continuum_core::controls::{apply_control_line, ControlCommand};
    use continuum_core::markov;
    use continuum_core::runtime::DesktopRuntime;
    use std::io::{self, BufRead};

    let args: Vec<String> = std::env::args().collect();

    if args.get(1).map(|s| s.as_str()) == Some("list") {
        println!("Available presets:");
        for name in markov::PRESET_NAMES {
            println!("  {}", name);
        }
        return;
    }

    let preset_name = args.get(1).map(|s| s.as_str()).unwrap_or("Ambient");
    let preset = markov::get_preset(preset_name);

    println!("[main] Continuum - preset: {}", preset.name);
    println!("[main] Layers: {}", preset.layers.len());
    println!("[main] Chords: {}", preset.chords.len());
    println!("[main] Base step: {:.0}ms", preset.base_step_ms);
    println!("[main] Controls: tempo/swing/instrument/stop");
    println!();

    let mut runtime = match DesktopRuntime::start(preset_name) {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("[audio] {}", err);
            return;
        }
    };
    let controls = runtime.controls();
    let stdin = io::stdin();

    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };

        match apply_control_line(controls.as_ref(), &line) {
            ControlCommand::Applied => {}
            ControlCommand::Stop => break,
            ControlCommand::Unknown => eprintln!("[controls] unknown command: {line}"),
        }
    }

    runtime.stop();
}

#[cfg(not(feature = "desktop-audio"))]
fn main() {
    eprintln!("The desktop runner requires the `desktop-audio` feature.");
}
