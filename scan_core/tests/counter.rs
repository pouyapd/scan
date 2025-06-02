use rand::{SeedableRng, rngs::SmallRng};
use scan_core::{program_graph::*, *};

#[test]
fn counter_pg() -> Result<(), PgError> {
    let mut rng = SmallRng::from_seed([0; 32]);
    let mut pg = ProgramGraphBuilder::new();
    let initial = pg.new_initial_location();
    let action = pg.new_action();
    let var = pg.new_var_with_rng(Expression::Const(Val::Integer(0)), &mut rng)?;
    pg.add_effect(
        action,
        var,
        Expression::Sum(vec![
            Expression::Var(var, Type::Integer),
            Expression::Const(Val::Integer(1)),
        ]),
    )
    .unwrap();
    for counter in 0..10 {
        let guard = Expression::Equal(Box::new((
            Expression::Var(var, Type::Integer),
            Expression::Const(Val::Integer(counter)),
        )));
        pg.add_transition(initial, action, initial, Some(guard))
            .unwrap();
    }
    let mut pg = pg.build();
    while let Some((action, post)) = pg
        .possible_transitions()
        .filter_map(|(a, iter)| {
            iter.into_iter()
                .map(|v| v.last())
                .collect::<Option<Vec<_>>>()
                .map(|l| (a, l))
        })
        .last()
    {
        assert!(pg.transition(action, post.as_slice(), &mut rng).is_ok());
    }
    Ok(())
}
