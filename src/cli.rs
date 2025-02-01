use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::PrintTrace;
use clap::Parser;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use scan_fmt_xml::scan_core::*;

/// A statistical model checker for large concurrent systems
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path of model's main XML file
    #[arg(value_hint = clap::ValueHint::DirPath, default_value = ".")]
    model: PathBuf,
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
    duration: Time,
    /// Saves execution traces in gz-compressed csv format
    #[arg(long = "save-traces", default_value = "false")]
    trace: bool,
}

impl Cli {
    pub fn run(&self) -> anyhow::Result<()> {
        let scxml_model = scan_fmt_xml::load(&self.model)?;
        let model_name = self
            .model
            .file_stem()
            .and_then(|s| s.to_str())
            .map_or("model".to_string(), |s| format!("'{s}'"));
        let confidence = self.confidence;
        let precision = self.precision;
        println!("SCANning {model_name} (target confidence {confidence}, precision {precision})");
        let run_status = scxml_model.model.run_status();
        let bar_state = run_status.clone();
        let bar = std::thread::spawn(move || print_progress_bar(confidence, precision, bar_state));
        if self.trace {
            std::fs::remove_dir_all("./traces").ok();
            std::fs::create_dir("./traces").expect("create traces dir");
            std::fs::create_dir("./traces/.temp").expect("create traces dir");
            std::fs::create_dir("./traces/success").expect("create success dir");
            std::fs::create_dir("./traces/failure").expect("create failure dir");
            std::fs::create_dir("./traces/undetermined").expect("create undetermined dir");
        }
        scxml_model.model.par_adaptive(
            confidence,
            precision,
            self.length,
            self.duration,
            self.trace.then_some(PrintTrace::new(&scxml_model)),
        );
        bar.join().expect("terminate bar process");
        let run_status = run_status.lock().expect("lock state");
        let rate =
            run_status.successes as f64 / (run_status.successes + run_status.failures) as f64;
        let mag = precision.log10().abs().ceil() as usize;
        println!(
            "Success rate {rate:.0$} ({1} runs with {2} successes, {3} failures)",
            mag,
            run_status.successes + run_status.failures,
            run_status.successes,
            run_status.failures,
        );
        Ok(())
    }
}

fn print_progress_bar(confidence: f64, precision: f64, bar_state: Arc<Mutex<RunStatus>>) {
    const FINE_BAR: &str = "█▉▊▋▌▍▎▏  ";
    let bound = okamoto_bound(confidence, precision);
    let style = ProgressStyle::with_template(
        "[{elapsed_precise}] {percent:>3}% [{wide_bar}] {msg} ETA: {eta:<5}",
    )
    .unwrap()
    .progress_chars(FINE_BAR);
    let bar = ProgressBar::new(bound.ceil() as u64)
        .with_style(style)
        .with_position(0)
        .with_message("Rate: N.A. (0/0)".to_string());
    let bars = MultiProgress::new();
    let bar = bars.add(bar);

    let mut bars_guarantees = Vec::new();
    let run_status = bar_state.lock().expect("lock state").clone();
    let name_len = run_status
        .guarantees
        .iter()
        .map(|(s, _)| s.len())
        .max()
        .unwrap_or_default();
    let prop_style = ProgressStyle::with_template(
        format!(
            "{{prefix:>{name_len}}} [{{wide_bar:.green.on_red}}] {{percent:>3}}% ({{msg}} fails)",
        )
        .as_str(),
    )
    .unwrap()
    .progress_chars(FINE_BAR);

    for (name, _) in run_status.guarantees.iter() {
        let bar = bars.add(
            ProgressBar::new(bound.ceil() as u64)
                .with_style(prop_style.clone())
                .with_position(0)
                .with_prefix(name.clone())
                .with_message("0".to_string()),
        );
        bars_guarantees.push(bar);
    }
    bars.set_move_cursor(true);
    // Magnitude of precision, to round results to sensible number of digits
    let mag = precision.log10().abs().ceil() as usize;
    loop {
        let run_status = bar_state.lock().expect("lock state").clone();
        if run_status.running {
            let runs = (run_status.successes + run_status.failures) as u64;
            let max_fail = run_status
                .guarantees
                .iter()
                .map(|(_, x)| *x)
                .max()
                .unwrap_or_default()
                .max(1);
            let digits = max_fail.to_string().chars().count();
            if runs > bar.position() {
                let avg = run_status.successes as f64 / runs as f64;
                let bound = adaptive_bound(avg, confidence, precision);
                let derived_precision =
                    derive_precision(run_status.successes, run_status.failures, confidence);
                bar.set_length(bound.ceil() as u64);
                bar.set_position(runs);
                bar.set_message(format!(
                    "Rate: {avg:.0$}±{derived_precision:.0$} ({1}/{2})",
                    mag, run_status.successes, run_status.failures
                ));
                // bar.tick();
                for (i, (_, guarantee)) in run_status.guarantees.into_iter().enumerate() {
                    let pos = runs - guarantee as u64;
                    let bar = &mut bars_guarantees[i];
                    bar.set_length(runs);
                    bar.set_position(pos);
                    bar.set_message(format!("{guarantee:<0$}", digits));
                    // bar.tick();
                }
            }
            // Sleep a while to limit update/refresh rate.
            std::thread::sleep(std::time::Duration::from_millis(100));
        } else {
            bar.finish_and_clear();
            break;
        }
    }
}
