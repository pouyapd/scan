// This file is the build_parser module itself. It defines the build_parser function
// and declares its sub-modules.

// Declare sub-modules
pub mod scxml;
pub mod data;
pub mod state;
pub mod transition;
pub mod executable;
pub mod send;
pub mod i_f;
pub mod param;
pub mod utils;

// Re-export main items from these sub-modules so they can be accessed via `build_parser::ItemName`
pub use scxml::Scxml;
pub use data::Data;
pub use state::State;
pub use transition::Transition;
pub use executable::Executable;
pub use send::Send;
pub use i_f::If;
pub use param::Param;
pub use utils::controll_expression;

// Import items needed within build_parser function itself
use crate::build_tree::tree;
use boa_interner::Interner;

// Import anyhow needed here
use anyhow::Result;
use anyhow::Context;

// Define the main build_parser function for this module
pub fn build_parser(s: tree::Tree, interner: &mut Interner) -> Result<Scxml> {
    let scxml_instance = scxml::Scxml::build_scxml(s, interner)
        .with_context(|| "Failed to build Scxml instance from tree structure")?;

    Ok(scxml_instance)
}