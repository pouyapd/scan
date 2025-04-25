//! Parser and model builder for SCAN's JANI specification format.

mod builder;
mod parser;
mod tracer;

use anyhow::Context;
pub use builder::JaniModelData;
use builder::build;
use log::info;
use parser::Model;
use scan_core::MtlOracle;
use scan_core::program_graph::{Action, PgError};
use scan_core::{PgModel, Scan};
use std::{fs::File, path::Path};
pub use tracer::TracePrinter;

pub type JaniScan = Scan<Action, PgError, PgModel, MtlOracle>;

pub fn load(path: &Path) -> anyhow::Result<(JaniScan, JaniModelData)> {
    info!(target: "parser", "parsing JANI model file '{}'", path.display());
    let reader = File::open(path)
        .with_context(|| format!("failed to create reader from file '{}'", path.display()))?;
    let jani_model: Model = serde_json::de::from_reader(reader).with_context(|| {
        format!(
            "failed to parse model specification in '{}'",
            path.display(),
        )
    })?;

    let (pg_model, oracle, jani_info) = build(jani_model)?;
    let scan = Scan::new(pg_model, oracle);

    Ok((scan, jani_info))
}
