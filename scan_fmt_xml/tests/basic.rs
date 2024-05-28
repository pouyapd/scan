use anyhow::anyhow;
use scan_fmt_xml::*;
use std::{path::PathBuf, str::FromStr};

const MAXSTEP: usize = 1000;

#[test]
fn fsm() -> anyhow::Result<()> {
    test(PathBuf::from_str("./tests/test_fsm/model.xml")?)
}

#[test]
fn datamodel() -> anyhow::Result<()> {
    test(PathBuf::from_str("./tests/test_datamodel/model.xml")?)
}

#[test]
fn enumdata() -> anyhow::Result<()> {
    test(PathBuf::from_str("./tests/test_enumdata/model.xml")?)
}

#[test]
fn send() -> anyhow::Result<()> {
    test(PathBuf::from_str("./tests/test_send/model.xml")?)
}

#[test]
fn send_onentry() -> anyhow::Result<()> {
    test(PathBuf::from_str("./tests/test_send_onentry/model.xml")?)
}

#[test]
fn origin() -> anyhow::Result<()> {
    test(PathBuf::from_str("./tests/test_origin/model.xml")?)
}

#[test]
fn origin_location() -> anyhow::Result<()> {
    test(PathBuf::from_str("./tests/test_origin_location/model.xml")?)
}

#[test]
fn param() -> anyhow::Result<()> {
    test(PathBuf::from_str("./tests/test_param/model.xml")?)
}

fn test(file: PathBuf) -> anyhow::Result<()> {
    let parser = Parser::parse(file)?;
    let mut model = Sc2CsVisitor::visit(parser)?;
    let mut steps = 0;
    assert!(!model.cs.possible_transitions().is_empty());
    while let Some((pg_id, act, loc)) = model.cs.possible_transitions().first().cloned() {
        model.cs.transition(pg_id, act, loc)?;
        steps += 1;
        if steps >= MAXSTEP {
            return Err(anyhow!("step limit reached"));
        }
    }
    Ok(())
}
