mod model;

pub use model::*;

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;

    #[test]
    fn test_model() {
        // Locations
        let mut location_id = 0;
        let initial = Location(location_id);
        location_id += 1;
        let left = Location(location_id);
        location_id += 1;
        let right = Location(location_id);
        // Actions
        let mut action_id = 0;
        let initialize = Action(action_id);
        action_id += 1;
        let move_left = Action(action_id);
        action_id += 1;
        let move_right = Action(action_id);
        // Variables
        let battery = Var(0);
        // Guards
        let out_of_charge = Formula::Less(Expr::Const(0), Expr::Var(battery));
        // Program graph definition
        let pg = ProgramGraph::new()
            .with_transition(initial, initialize, left, Formula::True)
            .with_effect(initialize, move |eval| {
                let _ = eval.insert(battery, Val::Integer(3));
            })
            .with_transition(left, move_right, right, out_of_charge.clone())
            .with_effect(move_right, move |eval| {
                let _ = eval.entry(battery).and_modify(|val| {
                    if let Val::Integer(int) = val {
                        *int -= 1;
                    }
                });
            })
            .with_transition(right, move_left, left, out_of_charge)
            .with_effect(move_left, move |eval| {
                let _ = eval.entry(battery).and_modify(|val| {
                    if let Val::Integer(int) = val {
                        *int -= 1;
                    }
                });
            });
        // Execution
        let pg = Rc::new(pg);
        let mut execution = Execution::new(initial, pg);
        execution.transition(initialize, left).expect("initialize");
        execution
            .transition(move_right, right)
            .expect("initialized left, battery = 3");
        execution
            .transition(move_right, right)
            .expect_err("already right");
        execution
            .transition(move_left, left)
            .expect("was right, battery = 2");
        execution
            .transition(move_right, right)
            .expect("was left, battery = 1");
        execution
            .transition(move_left, left)
            .expect_err("battery = 0");
    }
}
