use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::PrintTrace;
use clap::{Parser, ValueEnum};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use scan_fmt_xml::scan_core::{adaptive_bound, okamoto_bound, CsModelBuilder, RunStatus};

/// Supported model specification formats
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Format {
    /// SCXML format
    Scxml,
    /// JANI format
    Jani,
}

/// A statistical model checker for large concurrent systems
#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Path of model's main XML file
    #[arg(value_hint = clap::ValueHint::DirPath, default_value = ".")]
    model: PathBuf,
    /// Format used to specify the model
    #[arg(value_enum, short, long, default_value = "scxml")]
    format: Format,
    /// Confidence
    #[arg(short, long, default_value = "0.95")]
    confidence: f64,
    /// Precision or half-width parameter
    #[arg(short, long, default_value = "0.01")]
    precision: f64,
    /// Max length of execution trace
    #[arg(short, long, default_value = "1000000")]
    length: usize,
    /// Max duration of execution (in model-time)
    #[arg(short, long, default_value = "10000")]
    duration: u32,
    /// Saves execution traces in gz-compressed csv format
    #[arg(long = "save-traces", default_value = "false")]
    trace: bool,
    /// ASCII compatible output
    #[arg(long, default_value = "false")]
    ascii: bool,
}

impl Cli {
    pub fn run(&self) -> anyhow::Result<()> {
        match self.format {
            Format::Scxml => self.run_scxml(),
            Format::Jani => self.run_jani(),
        }
    }

    pub fn run_scxml(&self) -> anyhow::Result<()> {
        let scxml_model = scan_fmt_xml::load(&self.model)?;
        let model_name = self
            .model
            .file_stem()
            .and_then(|s| s.to_str())
            .map_or("model".to_string(), |s| format!("'{s}'"));
        let confidence = self.confidence;
        let precision = self.precision;
        // TODO: move this in Trace logic
        if self.trace {
            std::fs::remove_dir_all("./traces").ok();
            std::fs::create_dir("./traces").expect("create traces dir");
            std::fs::create_dir("./traces/.temp").expect("create traces dir");
        }
        let bar_state = Arc::clone(&scxml_model.model.run_status());
        let bar_model_name = model_name.clone();
        let ascii = self.ascii;
        let bar = std::thread::spawn(move || {
            print_progress_bar(bar_model_name, confidence, precision, bar_state, ascii)
        });
        scxml_model.model.par_adaptive(
            confidence,
            precision,
            self.length,
            self.duration,
            self.trace.then_some(PrintTrace::new(&scxml_model)),
        );
        bar.join().expect("terminate bar process");

        Ok(())
    }

    pub fn run_jani(&self) -> anyhow::Result<()> {
        use scan_fmt_jani::*;

        let jani_model = Parser::parse(self.model.as_path())?;
        let model = ModelBuilder::build(jani_model)?;
        let model = CsModelBuilder::new(model).build();
        let model_name = self
            .model
            .file_stem()
            .and_then(|s| s.to_str())
            .map_or("model".to_string(), |s| format!("'{s}'"));
        let confidence = self.confidence;
        let precision = self.precision;
        let bar_state = Arc::clone(&model.run_status());
        let bar_model_name = model_name.clone();
        let ascii = self.ascii;
        let bar = std::thread::spawn(move || {
            print_progress_bar(bar_model_name, confidence, precision, bar_state, ascii)
        });
        model.par_adaptive::<PrintTrace>(confidence, precision, self.length, self.duration, None);
        bar.join().expect("terminate bar process");

        Ok(())
    }
}

fn print_progress_bar(
    model_name: String,
    confidence: f64,
    precision: f64,
    bar_state: Arc<Mutex<RunStatus>>,
    ascii: bool,
) {
    const ARROW: &str = "⎯→ ";
    const ASCII_ARROW: &str = "-> ";
    const FINE_BAR: &str = "█▉▊▋▌▍▎▏  ";
    const ASCII_BAR: &str = "#  ";
    const ASCII_SPINNER: &str = "|/-\\";
    let run_status = bar_state.lock().expect("lock state").clone();

    let bars = MultiProgress::new();

    // Spinner
    let spinner_style = if ascii {
        ProgressStyle::with_template("{elapsed_precise} {spinner} {msg}")
            .unwrap()
            .tick_chars(ASCII_SPINNER)
    } else {
        ProgressStyle::with_template("{elapsed_precise} {spinner} {msg}").unwrap()
    };
    let spinner = ProgressBar::new_spinner()
        .with_style(spinner_style)
        .with_message(format!(
            "SCANning {model_name} (target confidence {confidence}, precision {precision})",
        ));
    let spinner = bars.add(spinner);

    // Progress bar
    let bound = okamoto_bound(confidence, precision).ceil() as u64;
    let progress_style =
        ProgressStyle::with_template("[{bar:50}] {percent:>3}% ({pos}/{len}) ETA: {eta}")
            .unwrap()
            .progress_chars(if ascii { ASCII_ARROW } else { ARROW });
    let progress_bar = ProgressBar::new(bound).with_style(progress_style);
    let progress_bar = bars.add(progress_bar);

    // Property bars
    let mut bars_guarantees = Vec::new();
    let prop_style = if ascii {
        ProgressStyle::with_template("[{bar:50}] {percent:>3}% {prefix} {msg}")
            .unwrap()
            .progress_chars(ASCII_BAR)
    } else {
        ProgressStyle::with_template("[{bar:50.green.on_red}] {percent:>3}% {prefix} {msg}")
            .unwrap()
            .progress_chars(FINE_BAR)
    };

    // Guarantees property bars
    for (name, _) in run_status.guarantees.iter() {
        let bar = bars.add(
            ProgressBar::new(1)
                .with_style(prop_style.clone())
                .with_position(1)
                .with_prefix(name.clone()),
        );
        bars_guarantees.push(bar);
    }

    // Overall property bar
    let overall_style = if ascii {
        ProgressStyle::with_template("[{bar:50}] {percent:>3}% {prefix} {msg}")
            .unwrap()
            .progress_chars(ASCII_BAR)
    } else {
        ProgressStyle::with_template("[{bar:50.blue.on_red}] {percent:>3}% {prefix} {msg}")
            .unwrap()
            .progress_chars(FINE_BAR)
    };
    let overall_bar = bars.add(
        ProgressBar::new(1)
            .with_style(overall_style)
            .with_position(1)
            .with_prefix("TOTAL"),
    );

    bars.set_move_cursor(true);
    loop {
        let run_status = bar_state.lock().expect("lock state").clone();
        let successes = run_status.successes;
        let failures = run_status.failures;
        let running = run_status.running;
        let runs = (successes + failures) as u64;
        let rate = successes as f64 / runs as f64;
        let guarantees = run_status.guarantees;
        if running {
            if runs > progress_bar.position() {
                // Status spinner
                spinner.tick();

                let bound = adaptive_bound(rate, confidence, precision);
                // let derived_precision =
                //     derive_precision(run_status.successes, run_status.failures, confidence);
                progress_bar.set_length(bound.ceil() as u64);
                progress_bar.set_position(runs);
                for (i, (_, guarantee)) in guarantees.iter().enumerate() {
                    let pos = runs - *guarantee as u64;
                    let bar = &mut bars_guarantees[i];
                    bar.set_position(pos);
                    bar.set_length(runs);
                    if *guarantee > 0 {
                        bar.set_message(format!("({guarantee} failed)"));
                    }
                }

                // Overall property bar
                let overall = guarantees.iter().map(|(_, g)| *g).sum::<u32>() as u64;
                let pos = runs - overall;
                overall_bar.set_position(pos);
                overall_bar.set_length(runs);
                if overall > 0 {
                    overall_bar.set_message(format!("({overall} failed)"));
                }
            }
        } else {
            bars.set_move_cursor(false);
            spinner.finish_and_clear();
            progress_bar.finish_and_clear();
            bars_guarantees.iter().for_each(|b| b.finish_and_clear());
            overall_bar.finish_and_clear();
            // Magnitude of precision, to round results to sensible number of digits
            let mag = (precision.log10().abs().ceil() as usize).max(2);
            println!(
                "SCAN results for {model_name} (confidence {confidence}, precision {precision})"
            );
            println!("Completed {runs} runs with {successes} successes, {failures} failures)");
            for (name, f) in guarantees.into_iter() {
                print!(
                    "{name} success rate: {0:.1$}",
                    ((runs - f as u64) as f64) / (runs as f64),
                    mag,
                );
                if f > 0 {
                    println!(" ({f} fails)");
                } else {
                    println!();
                }
            }
            println!("Overall success rate: {rate:.0$}", mag);
            break;
        }
        // Sleep a while to limit update/refresh rate.
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
