
use std::path::{Path, PathBuf};
use boa_interner::Interner;
use std::any::TypeId;
use anyhow::Result;

#[test]
fn datamodel() ->Result<()>{
    test(Path::new("tests/assets/test_datamodel/fsm.scxml"))
}

#[test]
fn elif() ->Result<()>{
    test(Path::new("tests/assets/test_elif/fsm.scxml"))
}

#[test]
fn enumdata() ->Result<()>{
    test(Path::new("tests/assets/test_enumdata/fsm.scxml"))
}

#[test]
fn fsm() ->Result<()>{
    test(Path::new("tests/assets/test_fsm/fsm.scxml"))
}

#[test]
fn tif() ->Result<()>{
    test(Path::new("tests/assets/test_if/fsm.scxml"))
}

#[test]
fn origin_1() ->Result<()>{
    test(Path::new("tests/assets/test_origin/fsm_1.scxml"))
}

#[test]
fn origin_2() ->Result<()>{
    test(Path::new("tests/assets/test_origin/fsm_2.scxml"))
}

#[test]
fn origin_location_1() ->Result<()>{
    test(Path::new("tests/assets/test_origin_location/fsm_1.scxml"))
}

#[test]
fn origin_location_2() ->Result<()>{
    test(Path::new("tests/assets/test_origin_location/fsm_2.scxml"))
}

#[test]
fn param_1() ->Result<()>{
    test(Path::new("tests/assets/test_param/fsm_1.scxml"))
}

#[test]
fn param_2() ->Result<()>{
    test(Path::new("tests/assets/test_param/fsm_2.scxml"))
}

#[test]
fn param_tennis_1() ->Result<()>{
    test(Path::new("tests/assets/test_param_tennis/fsm_1.scxml"))
}

#[test]
fn param_tennis_2() ->Result<()>{
    test(Path::new("tests/assets/test_param_tennis/fsm_2.scxml"))
}

#[test]
fn param_triangle_1() ->Result<()>{
    test(Path::new("tests/assets/test_param_triangle/fsm_1.scxml"))
}

#[test]
fn param_triangle_2() ->Result<()>{
    test(Path::new("tests/assets/test_param_triangle/fsm_2.scxml"))
}

#[test]
fn param_triangle_3() ->Result<()>{
    test(Path::new("tests/assets/test_param_triangle/fsm_3.scxml"))
}

#[test]
fn send_1() ->Result<()>{
    test(Path::new("tests/assets/test_send/fsm_1.scxml"))
}

#[test]
fn send_2() ->Result<()>{
    test(Path::new("tests/assets/test_send/fsm_2.scxml"))
}

#[test]
fn send_onentry_1() ->Result<()>{
    test(Path::new("tests/assets/test_send_onentry/fsm_1.scxml"))
}

#[test]
fn send_onentry_2() ->Result<()>{
    test(Path::new("tests/assets/test_send_onentry/fsm_2.scxml"))
}

#[test]
fn send_triangle_1() ->Result<()>{
    test(Path::new("tests/assets/test_send_triangle/fsm_1.scxml"))
}

#[test]
fn send_triangle_2() ->Result<()>{
    test(Path::new("tests/assets/test_send_triangle/fsm_2.scxml"))
}

#[test]
fn send_triangle_3() ->Result<()>{
    test(Path::new("tests/assets/test_send_triangle/fsm_3.scxml"))
}

fn test(path: &Path) ->Result<()>{
    let mut interner_c = Interner::new();
    let scxml_instance = scxml_lib::build(path.to_owned(), &mut interner_c)?;
    Ok(())
}