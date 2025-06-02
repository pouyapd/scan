use std::path::Path;

#[test]
fn jani_test() {
    test(Path::new("./tests/test.jani"))
}

#[test]
fn jani_test2() {
    test(Path::new("./tests/test2.jani"))
}

#[test]
fn workflow() {
    test(Path::new("./tests/workflow.jani"))
}

#[test]
fn brp() {
    test(Path::new("./tests/brp.v1.jani"))
}

#[test]
fn crowds() {
    test(Path::new("./tests/crowds.v1.jani"))
}

fn test(path: &Path) {
    let (scan, ..) = scan_jani::load(path).expect("load");
    scan.adaptive::<scan_jani::TracePrinter>(0.95, 0.01, 10000, None);
}
