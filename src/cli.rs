use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::PrintTrace;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
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
    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        let scxml_model = scan_fmt_xml::load(&self.model)?;
        let model_name = self
            .model
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("model");
        let confidence = self.confidence;
        let precision = self.precision;
        println!("SCANning '{model_name}' (confidence {confidence}; precision {precision})");
        let run_state = Arc::new(Mutex::new((0, 0, true)));
        let bar_state = run_state.clone();
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
            &scxml_model.guarantees,
            &scxml_model.assumes,
            confidence,
            precision,
            self.length,
            self.duration,
            self.trace.then_some(PrintTrace::new(&scxml_model)),
            run_state.clone(),
        );
        bar.join().expect("terminate bar process");
        let (s, f, running) = *run_state.lock().expect("lock state");
        assert!(!running);
        println!("Completed {} runs with {s} successes, {f} failures", s + f);
        let rate = s as f64 / (s + f) as f64;
        let mag = precision.log10().abs().ceil() as usize;
        println!(
            "Success rate {rate:.0$}±{precision} (confidence {confidence})",
            mag
        );
        Ok(())
    }
}

fn print_progress_bar(confidence: f64, precision: f64, bar_state: Arc<Mutex<(u32, u32, bool)>>) {
    const FINE_BAR: &str = "█▉▊▋▌▍▎▏  ";
    let avg = 0.5f64;
    let mut bound = adaptive_bound(avg, confidence, precision);
    let style = ProgressStyle::with_template(
        "[{elapsed_precise}] {percent:>2}% {wide_bar} {msg} ETA: {eta:<5}",
    )
    .unwrap()
    .progress_chars(FINE_BAR);
    let bar = ProgressBar::new(bound.ceil() as u64).with_style(style);
    bar.set_position(0);
    // Magnitude of precision, to round results to sensible number of digits
    let mag = precision.log10().abs().ceil() as usize;
    bar.set_message(format!("Success rate: {avg:.0$}", mag));
    bar.tick();
    // bar.enable_steady_tick(Duration::from_millis(32));
    // bar.update(|state| {
    //     if RUNNING.load(Ordering::Relaxed) {
    //         let s = SUCCESSES.load(Ordering::Relaxed);
    //         let f = FAILURES.load(Ordering::Relaxed);
    //         if s + f > bar.position() as u32 {
    //             let runs = s + f;
    //             let avg = s as f64 / runs as f64;
    //             bound = adaptive_bound(avg, confidence, precision);
    //             state.set_len(bound.ceil() as u64);
    //             state.set_pos(runs as u64);
    //             // let derived_precision = derive_precision(s, f, confidence);
    //         }
    //     } else {
    //         state.set_pos(state.len().unwrap_or(1));
    //     }
    // });
    let (mut s, mut f, mut running) = *bar_state.lock().expect("lock state");
    while running {
        if s + f > bar.position() as u32 {
            let runs = s + f;
            let avg = s as f64 / runs as f64;
            bound = adaptive_bound(avg, confidence, precision);
            bar.set_length(bound.ceil() as u64);
            bar.set_position(runs as u64);
            let derived_precision = derive_precision(s, f, confidence);
            bar.set_message(format!(
                "Success rate: {avg:.0$}±{derived_precision:.0$}",
                mag
            ));
        }
        bar.tick();
        // Sleep a while to limit update/refresh rate.
        std::thread::sleep(std::time::Duration::from_millis(32));
        (s, f, running) = *bar_state.lock().expect("lock state");
    }
    bar.finish_and_clear();
}
