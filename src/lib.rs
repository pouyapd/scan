//! SCAN (StoChastic ANalyzer) is a statistical model checker developed in the context of the CONVINCE project.
//! Multiple specification formats are (planned to be) accepted:
//!
//! - [x] SCXML (subset of) to specify State Charts model
//! - [ ] Promela
//! - [ ] JANI
//!
//! Internally, SCAN uses the formalism of Channel Systems for modelling,
//! and Metric Temporal Logic (MTL) to specify properties.

// #![warn(missing_docs)]
// TODO list:
// - [ ] use fast hasher for hashmap and hashset
// - [ ] smallvec optimization
// - [ ] multi-threading

pub use scan_fmt_xml;
