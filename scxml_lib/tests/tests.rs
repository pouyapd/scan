// No extern crate needed for integration tests in tests/

use scxml_lib::build; // Import the build function from the lib crate
use std::path::Path; // Only Path is needed
use boa_interner::Interner;
// use std::any::TypeId; // Removed unused import
use anyhow::Result;
// use std::path::PathBuf; // Removed unused import


// Paths are relative to the crate root (scxml_lib), should point to files in scxml_lib/tests/
#[test]
fn datamodel() ->Result<()>{
    test(Path::new("tests/test_datamodel/fsm.scxml"))
}

#[test]
fn elif() ->Result<()>{
    test(Path::new("tests/test_elif/fsm.scxml"))
}

#[test]
fn enumdata() ->Result<()>{
    test(Path::new("tests/test_enumdata/fsm.scxml"))
}

#[test]
fn fsm() ->Result<()>{
    test(Path::new("tests/test_fsm/fsm.scxml"))
}

#[test]
fn tif() ->Result<()>{
    test(Path::new("tests/test_if/fsm.scxml"))
}

#[test]
fn origin_1() ->Result<()>{
    test(Path::new("tests/test_origin/fsm_1.scxml"))
}

#[test]
fn origin_2() ->Result<()>{
    test(Path::new("tests/test_origin/fsm_2.scxml"))
}

#[test]
fn origin_location_1() ->Result<()>{
    test(Path::new("tests/test_origin_location/fsm_1.scxml"))
}

#[test]
fn origin_location_2() ->Result<()>{
    test(Path::new("tests/test_origin_location/fsm_2.scxml"))
}

#[test]
fn param_1() ->Result<()>{
    test(Path::new("tests/test_param/fsm_1.scxml"))
}

#[test]
fn param_2() ->Result<()>{
    test(Path::new("tests/test_param/fsm_2.scxml"))
}

#[test]
fn param_tennis_1() ->Result<()>{
    test(Path::new("tests/test_param_tennis/fsm_1.scxml"))
}

#[test]
fn param_tennis_2() ->Result<()>{
    test(Path::new("tests/test_param_tennis/fsm_2.scxml"))
}

#[test]
fn param_triangle_1() ->Result<()>{
    test(Path::new("tests/test_param_triangle/fsm_1.scxml"))
}

#[test]
fn param_triangle_2() ->Result<()>{
    test(Path::new("tests/test_param_triangle/fsm_2.scxml"))
}

#[test]
fn param_triangle_3() ->Result<()>{
    test(Path::new("tests/test_param_triangle/fsm_3.scxml"))
}

#[test]
fn send_1() ->Result<()>{
    test(Path::new("tests/test_send/fsm_1.scxml"))
}

#[test]
fn send_2() ->Result<()>{
    test(Path::new("tests/test_send/fsm_2.scxml"))
}

#[test]
fn send_onentry_1() ->Result<()>{
    test(Path::new("tests/test_send_onentry/fsm_1.scxml"))
}

#[test]
fn send_onentry_2() ->Result<()>{
    test(Path::new("tests/test_send_onentry/fsm_2.scxml"))
}

#[test]
fn send_triangle_1() ->Result<()>{
    test(Path::new("tests/test_send_triangle/fsm_1.scxml"))
}

#[test]
fn send_triangle_2() ->Result<()>{
    test(Path::new("tests/test_send_triangle/fsm_2.scxml"))
}

#[test]
fn send_triangle_3() ->Result<()>{
    test(Path::new("tests/test_send_triangle/fsm_3.scxml"))
}


fn test(path: &Path) ->Result<()>{
    let mut interner_c = Interner::new();
    // build is imported directly now and returns Result<Scxml>
    // The ? operator works because build returns Result
    let scxml_instance = build(path.to_owned(), &mut interner_c)?;

    // Add assertions here if needed to check the content of scxml_instance on success
    // assert!(scxml_instance.some_property == some_value);

    Ok(()) // Test passes if build succeeds and no assertions fail
}