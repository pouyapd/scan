use anyhow::anyhow;
use scan::*;
use scan_fmt_xml::Parser;
use std::{path::PathBuf, str::FromStr};

const MAXSTEP: usize = 1000;

#[test]
fn fsm() -> anyhow::Result<()> {
    test(PathBuf::from_str("./tests/test_1/model.xml")?)
}

#[test]
fn datamodel() -> anyhow::Result<()> {
    test(PathBuf::from_str("./tests/test_2/model.xml")?)
}

fn test(file: PathBuf) -> anyhow::Result<()> {
    let parser = Parser::parse(file)?;
    let mut model = parser.build_model();
    let mut steps = 0;
    assert!(!model.possible_transitions().is_empty());
    while let Some((pg_id, act, loc)) = model.possible_transitions().first().cloned() {
        model.transition(pg_id, act, loc)?;
        steps += 1;
        if steps >= MAXSTEP {
            return Err(anyhow!("step limit reached"));
        }
    }
    Ok(())
}
