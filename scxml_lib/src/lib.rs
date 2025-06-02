pub mod build_tree;
pub mod build_parser;

use std::path::PathBuf;

use boa_interner::Interner;

pub use build_tree::build_tree;
pub use crate::build_parser::{build_parser, Scxml};

use anyhow::Result;
use anyhow::Context;

pub fn build(s: PathBuf, interner: &mut Interner) -> Result<Scxml> {
    let collection_trees = build_tree(s.clone())
        .with_context(|| format!("Failed to build trees from path: {}", s.display()))?;

    let scxml_instance = build_parser(collection_trees, interner)
        .with_context(|| "Failed to build parser collection")?;

    Ok(scxml_instance)
}