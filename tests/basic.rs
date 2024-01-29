use quick_xml::Reader;
use scan::Parser;
use std::{error::Error, path::PathBuf, str::FromStr};

#[test]
fn empty() -> Result<(), Box<dyn Error>> {
    let file = PathBuf::from_str("./assets/tests/empty.scxml")?;
    let mut reader = Reader::from_file(file)?;
    let model = Parser::parse(&mut reader)?;
    assert!(model.possible_transitions().is_empty());
    Ok(())
}

#[test]
fn trivial() -> Result<(), Box<dyn Error>> {
    let file = PathBuf::from_str("./assets/tests/trivial.scxml")?;
    let mut reader = Reader::from_file(file)?;
    let model = Parser::parse(&mut reader)?;
    assert!(model.possible_transitions().is_empty());
    Ok(())
}

#[test]
fn transition() -> Result<(), Box<dyn Error>> {
    let file = PathBuf::from_str("./assets/tests/transition.scxml")?;
    let mut reader = Reader::from_file(file)?;
    let mut model = Parser::parse(&mut reader)?;
    for _ in 0..2 {
        assert_eq!(model.possible_transitions().len(), 1);
        let (pg_id, action, post) = *model.possible_transitions().first().expect("len 1");
        model.transition(pg_id, action, post)?;
    }
    assert!(model.possible_transitions().is_empty());
    Ok(())
}

#[test]
fn variable() -> Result<(), Box<dyn Error>> {
    let file = PathBuf::from_str("./assets/tests/variable.scxml")?;
    let mut reader = Reader::from_file(file)?;
    let mut model = Parser::parse(&mut reader)?;
    for _ in 0..3 {
        assert_eq!(model.possible_transitions().len(), 1);
        let (pg_id, action, post) = *model.possible_transitions().first().expect("len 1");
        model.transition(pg_id, action, post)?;
    }
    assert!(model.possible_transitions().is_empty());
    Ok(())
}
