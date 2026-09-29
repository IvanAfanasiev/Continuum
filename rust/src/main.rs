//! Настольный раннер: CLI поверх [`rust_lib_cont::playback::PlaybackController`].
//!
//! Использование (двойной дефис — синтаксис самого cargo, он отделяет
//! аргументы программы от аргументов cargo и убрать его нельзя):
//! ```text
//! cargo run -- lofi            # стиль lofi, сид по умолчанию
//! cargo run -- 1234            # сид 1234, стиль club
//! cargo run -- 1234 lofi       # сид + стиль (порядок свободный)
//! cargo run -- --style=lofi --seed=7
//! cargo run -- lofi --render 64  # офлайн-проверка без аудиоустройства
//! cargo run -- club --soak 60    # прогон всего тракта 60 секунд аудио
//! ```

use std::{
    env, thread,
    time::{Duration, Instant},
};

use rust_lib_cont::core::DEFAULT_SEED;
use rust_lib_cont::playback::{
    render_offline, soak_render, PlaybackController, RenderReport, SoakReport,
};
use rust_lib_cont::styles;

const DEFAULT_RENDER_STEPS: usize = 64;
const DEFAULT_SOAK_SECS: f32 = 60.0;

#[derive(Debug, PartialEq)]
struct Cli {
    seed: u64,
    style: String,
    render_steps: Option<usize>,
    soak_secs: Option<f32>,
}

impl Cli {
    fn new() -> Self {
        Self {
            seed: DEFAULT_SEED,
            style: "club".to_string(),
            render_steps: None,
            soak_secs: None,
        }
    }
}

/// Разбор аргументов: число = сид, остальное = имя стиля.
/// Неизвестные имена НЕ падают и НЕ молчат — о них сообщает вызывающий.
fn parse_cli(args: &[String]) -> Cli {
    let mut cli = Cli::new();
    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--render" {
            match args.get(i + 1) {
                Some(next) if !next.starts_with("--") => {
                    if let Ok(n) = next.parse::<usize>() {
                        cli.render_steps = Some(n);
                        i += 1;
                    } else {
                        // Следом идет стиль/сид — рендерим дефолтное число шагов.
                        cli.render_steps = Some(DEFAULT_RENDER_STEPS);
                    }
                }
                _ => cli.render_steps = Some(DEFAULT_RENDER_STEPS),
            }
        } else if let Some(value) = arg.strip_prefix("--render=") {
            cli.render_steps = value.parse().ok();
        } else if arg == "--soak" {
            match args.get(i + 1) {
                Some(next) if !next.starts_with("--") => {
                    if let Ok(n) = next.parse::<f32>() {
                        cli.soak_secs = Some(n);
                        i += 1;
                    } else {
                        cli.soak_secs = Some(DEFAULT_SOAK_SECS);
                    }
                }
                _ => cli.soak_secs = Some(DEFAULT_SOAK_SECS),
            }
        } else if let Some(value) = arg.strip_prefix("--soak=") {
            cli.soak_secs = value.parse().ok();
        } else if let Some(value) = arg.strip_prefix("--seed=") {
            if let Ok(n) = value.parse::<u64>() {
                cli.seed = n;
            }
        } else if let Some(value) = arg.strip_prefix("--style=") {
            cli.style = value.to_string();
        } else if arg == "--help" || arg == "-h" {
            print_help();
            std::process::exit(0);
        } else if let Ok(n) = arg.parse::<u64>() {
            cli.seed = n;
        } else {
            cli.style = arg.to_string();
        }
        i += 1;
    }
    cli
}

fn print_help() {
    println!(
        "Usage: cargo run -- [SEED] [STYLE] [--render [N]] [--soak [SECS]]\n\
         Styles: {}\n\
         Examples:\n  \
         cargo run -- lofi\n  \
         cargo run -- 1234 deephouse\n  \
         cargo run -- lofi --render 64\n  \
         cargo run -- club --soak 300",
        styles::style_names().join(", ")
    );
}

fn print_soak(report: &SoakReport, wall_secs: f32) {
    println!(
        "soak: {} (seed {}), audio {:.1}s, samples: {}, notes: {}",
        report.style_name, report.seed, report.seconds, report.samples, report.notes
    );
    println!(
        "  peak: {:.3}, rms: {:.3}, non-finite: {}",
        report.peak, report.rms, report.non_finite
    );
    println!(
        "  wall: {:.1}s ({:.0}x realtime), memory: bounded by construction",
        wall_secs,
        report.seconds / wall_secs.max(0.001)
    );
}

fn print_report(report: &RenderReport) {
    println!(
        "style: {} ({} BPM), seed: {}, steps: {}, notes: {}",
        report.style_name, report.bpm, report.seed, report.steps, report.notes_total
    );
    for (id, count) in &report.notes_per_instrument {
        println!("  instrument {id}: {count} notes");
    }
    println!("  first notes (step, inst, pitch, vel):");
    for n in &report.first_notes {
        println!(
            "    step {:>3}  inst {}  pitch {:>3}  vel {:.2}",
            n.step, n.instrument_id, n.pitch, n.velocity
        );
    }
}

fn main() {
    let cli = parse_cli(&env::args().collect::<Vec<_>>());

    // Имя нормализуем через реестр: "Club", " LOFI " и т.п. тоже находятся.
    // Неизвестное имя — предупреждение + club, а не тихий чужой стиль.
    let style_name = match styles::make_style(&cli.style) {
        Some(s) => s.name().to_string(),
        None => {
            eprintln!(
                "Unknown style '{}'. Available: {}. Falling back to club.",
                cli.style,
                styles::style_names().join(", ")
            );
            "club".to_string()
        }
    };

    if let Some(steps) = cli.render_steps {
        match render_offline(&style_name, cli.seed, steps) {
            Some(report) => print_report(&report),
            None => eprintln!("Cannot render unknown style '{style_name}'"),
        }
        return;
    }

    if let Some(secs) = cli.soak_secs {
        let wall = Instant::now();
        match soak_render(&style_name, cli.seed, secs, 44_100.0) {
            Some(report) => print_soak(&report, wall.elapsed().as_secs_f32()),
            None => eprintln!("Cannot render unknown style '{style_name}'"),
        }
        return;
    }

    let controller = PlaybackController::new();
    let status = controller.start(&style_name, cli.seed, true);
    println!(
        "Playing '{}' (seed {}). Ctrl+C to quit.",
        status.style_name, status.seed
    );

    // Главная петля CLI: просто ждет. Выход — только по ошибке движка.
    loop {
        thread::sleep(Duration::from_secs(1));
        let status = controller.status();
        if let Some(err) = status.engine_error {
            eprintln!("[Engine Error] {err}");
            controller.stop();
            std::process::exit(1);
        }
        if !status.running {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        std::iter::once("bin".to_string())
            .chain(words.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn single_style_arg() {
        let cli = parse_cli(&args(&["lofi"]));
        assert_eq!(cli.style, "lofi");
        assert_eq!(cli.seed, DEFAULT_SEED);
    }

    #[test]
    fn single_seed_arg() {
        let cli = parse_cli(&args(&["1234"]));
        assert_eq!(cli.seed, 1234);
        assert_eq!(cli.style, "club");
    }

    #[test]
    fn seed_and_style_in_any_order() {
        let a = parse_cli(&args(&["1234", "lofi"]));
        let b = parse_cli(&args(&["lofi", "1234"]));
        assert_eq!(a.seed, 1234);
        assert_eq!(a.style, "lofi");
        assert_eq!(a, b);
    }

    #[test]
    fn render_flag_with_and_without_value() {
        assert_eq!(
            parse_cli(&args(&["lofi", "--render", "32"])).render_steps,
            Some(32)
        );
        assert_eq!(
            parse_cli(&args(&["--render"])).render_steps,
            Some(DEFAULT_RENDER_STEPS)
        );
        assert_eq!(
            parse_cli(&args(&["--render", "lofi"])).render_steps,
            Some(DEFAULT_RENDER_STEPS)
        );
        assert_eq!(parse_cli(&args(&["--render=16"])).render_steps, Some(16));
    }

    #[test]
    fn soak_flag_with_and_without_value() {
        assert_eq!(
            parse_cli(&args(&["club", "--soak", "30"])).soak_secs,
            Some(30.0)
        );
        assert_eq!(
            parse_cli(&args(&["--soak"])).soak_secs,
            Some(DEFAULT_SOAK_SECS)
        );
        assert_eq!(parse_cli(&args(&["--soak=10"])).soak_secs, Some(10.0));
    }
}
