use std::collections::HashMap;

use anyhow::Ok;

use crate::{parser::*, ChannelSystem, ChannelSystemBuilder, CsFormula, PgId};

#[derive(Debug)]
pub struct CsModel {
    cs: ChannelSystem,
    skill_ids: HashMap<String, PgId>,
    skill_names: HashMap<PgId, String>,
    component_ids: HashMap<String, PgId>,
    component_names: HashMap<PgId, String>,
}

impl CsModel {
    pub fn build(parser: Parser) -> anyhow::Result<Self> {
        let mut cs = ChannelSystemBuilder::new();
        let mut skill_ids = HashMap::new();
        let mut skill_names = HashMap::new();
        let mut component_ids = HashMap::new();
        let mut component_names = HashMap::new();

        for (name, declaration) in parser.skill_list.iter() {
            if let MoC::Fsm(fsm) = &declaration.moc {
                let mut states = HashMap::new();
                let mut actions = HashMap::new();
                let pg_id = skill_ids.get(name).cloned().unwrap_or_else(|| {
                    let pg_id = cs.new_program_graph();
                    skill_ids.insert(name.to_owned(), pg_id);
                    skill_names.insert(pg_id, name.to_owned());
                    pg_id
                });
                let initial = cs.initial_location(pg_id)?;
                states.insert(fsm.initial.to_owned(), initial);
                for (state_name, state) in fsm.states.iter() {
                    let state_id = if let Some(state_id) = states.get(state_name) {
                        *state_id
                    } else {
                        let state_id = cs.new_location(pg_id)?;
                        states.insert(state_name.to_owned(), state_id);
                        state_id
                    };
                    for transition in state.transitions.iter() {
                        let target_id = if let Some(state_id) = states.get(state_name) {
                            *state_id
                        } else {
                            let state_id = cs.new_location(pg_id)?;
                            states.insert(state_name.to_owned(), state_id);
                            state_id
                        };
                        // WARN This is NOT the right way to deal with the NULL event!
                        // TODO: Fix it!
                        let event: String =
                            transition.event.to_owned().unwrap_or(String::from("NULL"));
                        let action = if let Some(action) = actions.get(&event) {
                            *action
                        } else {
                            let action = cs.new_action(pg_id)?;
                            actions.insert(event, action);
                            action
                        };
                        let guard = CsFormula::new_true(pg_id);
                        cs.add_transition(pg_id, state_id, action, target_id, guard)?;
                    }
                }
            }
        }

        let cs = cs.build();
        let model = CsModel {
            cs,
            skill_ids,
            skill_names,
            component_ids,
            component_names,
        };
        Ok(model)
    }
}
