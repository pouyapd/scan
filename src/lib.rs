mod model;

pub use model::*;

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;

    #[test]
    fn program_graph() {
        // Variables
        let battery = Var(0);
        // Create Program Graph
        let mut pg = ProgramGraph::new();
        // Locations
        let initial = pg.new_location();
        let left = pg.new_location();
        let center = pg.new_location();
        let right = pg.new_location();
        // Actions
        let initialize = pg.new_action(move |eval| {
            let _ = eval.insert(battery, Val::Integer(3));
        });
        let move_left = pg.new_action(move |eval| {
            let _ = eval.entry(battery).and_modify(|val| {
                if let Val::Integer(int) = val {
                    *int -= 1;
                }
            });
        });
        let move_right = pg.new_action(move |eval| {
            let _ = eval.entry(battery).and_modify(|val| {
                if let Val::Integer(int) = val {
                    *int -= 1;
                }
            });
        });
        // Guards
        let out_of_charge = Formula::Less(Expr::Const(0), Expr::Var(battery));
        // Program graph definition
        pg.add_transition(initial, initialize, center, Formula::True)
            .expect("legal transition");
        pg.add_transition(left, move_right, center, out_of_charge.clone())
            .expect("legal transition");
        pg.add_transition(center, move_right, right, out_of_charge.clone())
            .expect("legal transition");
        pg.add_transition(right, move_left, center, out_of_charge.clone())
            .expect("legal transition");
        pg.add_transition(center, move_left, left, out_of_charge)
            .expect("legal transition");
        // Execution
        let pg = Rc::new(pg);
        let mut execution = Execution::new(initial, pg);
        assert_eq!(execution.possible_transitions().unwrap().len(), 1);
        execution
            .transition(initialize, center)
            .expect("initialize");
        assert_eq!(execution.possible_transitions().unwrap().len(), 2);
        execution
            .transition(move_right, right)
            .expect("initialized left, battery = 3");
        assert_eq!(execution.possible_transitions().unwrap().len(), 1);
        execution
            .transition(move_right, right)
            .expect_err("already right");
        assert_eq!(execution.possible_transitions().unwrap().len(), 1);
        execution
            .transition(move_left, center)
            .expect("was right, battery = 2");
        assert_eq!(execution.possible_transitions().unwrap().len(), 2);
        execution
            .transition(move_left, left)
            .expect("was center, battery = 1");
        assert_eq!(execution.possible_transitions().unwrap().len(), 0);
        execution
            .transition(move_left, left)
            .expect_err("battery = 0");
    }
}
