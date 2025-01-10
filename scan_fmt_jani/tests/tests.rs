use std::path::Path;

const MAXSTEP: usize = 1000;

#[test]
fn jani_test() {
    test(Path::new("./tests/test.jani"))
}

#[test]
fn jani_test2() {
    test(Path::new("./tests/test2.jani"))
}

// FAIL: Feature not supported: probability
// #[test]
// fn die() {
//     test(Path::new("./tests/die.jani"))
// }

// FAIL: Feature not supported: probability
// #[test]
// fn dining_crypt3() {
//     test(Path::new("./tests/dining_crypt3.jani"))
// }

fn test(path: &Path) {
    let ast = scan_fmt_jani::Parser::parse(path).unwrap();
    let mut model = scan_fmt_jani::ModelBuilder::build(ast).unwrap();
    let mut steps = 0;
    assert!(model.possible_transitions().count() > 0);
    while let Some((pg_id, act, loc)) = model
        .possible_transitions()
        .take(1)
        .collect::<Vec<_>>()
        .pop()
    {
        model.transition(pg_id, act, loc).unwrap();
        steps += 1;
        assert!(steps < MAXSTEP, "step limit reached");
    }
}
