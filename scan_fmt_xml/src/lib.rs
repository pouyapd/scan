//! Parser and model builder for SCAN's CONVINCE-XML specification format.

mod builder;
mod parser;
mod print_trace;

use std::path::Path;

pub use builder::ScxmlModel;
pub use print_trace::TracePrinter;
pub use scan_core;

pub fn load(path: &Path) -> anyhow::Result<(scan_core::CsModel, ScxmlModel)> {
    let parser = parser::Parser::parse(path)?;
    builder::ModelBuilder::build(parser)
}
