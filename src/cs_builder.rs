use std::collections::HashMap;

use anyhow::Ok;
use clap::Id;
use log::info;

use crate::{
    parser::*, ChannelSystem, ChannelSystemBuilder, CsFormula, CsIntExpr, Integer, PgId, VarType,
};

#[derive(Debug)]
pub struct CsModel {
    cs: ChannelSystem,
    skill_ids: HashMap<String, PgId>,
    skill_names: HashMap<PgId, String>,
    component_ids: HashMap<String, PgId>,
    component_names: HashMap<PgId, String>,
}

#[derive(Debug)]
pub struct Sc2CsVisitor {
    cs: ChannelSystemBuilder,
    skill_ids: HashMap<String, PgId>,
    component_ids: HashMap<String, PgId>,
    events: HashMap<String, Integer>,
}

impl Sc2CsVisitor {
    pub fn visit(parser: Parser) -> anyhow::Result<CsModel> {
        let cs = ChannelSystemBuilder::new();
        let skill_ids = HashMap::new();
        let component_ids = HashMap::new();
        let mut model = Sc2CsVisitor {
            cs,
            skill_ids,
            component_ids,
            events: HashMap::new(),
        };

        info!("Visit skill list");
        for (name, declaration) in parser.skill_list.iter() {
            if let MoC::Fsm(fsm) = &declaration.moc {
                let pg_id = model.skill_ids.get(name).cloned().unwrap_or_else(|| {
                    let pg_id = model.cs.new_program_graph();
                    model.skill_ids.insert(name.to_owned(), pg_id);
                    pg_id
                });
                info!("Visit skill {name}");
                model.build_fsm(fsm, pg_id)?;
            }
        }

        info!("Visit component list");
        for (name, declaration) in parser.component_list.iter() {
            if let MoC::Fsm(fsm) = &declaration.moc {
                let pg_id = model.component_ids.get(name).cloned().unwrap_or_else(|| {
                    let pg_id = model.cs.new_program_graph();
                    model.component_ids.insert(name.to_owned(), pg_id);
                    pg_id
                });
                info!("Visit component {name}");
                model.build_fsm(fsm, pg_id)?;
            }
        }

        let model = model.build();

        Ok(model)
    }

    fn build_fsm(&mut self, fsm: &Fsm, pg_id: PgId) -> anyhow::Result<()> {
        // Initialize fsm
        let mut states = HashMap::new();
        let initial = self.cs.initial_location(pg_id)?;
        states.insert(fsm.initial.to_owned(), initial);
        let true_cond = CsFormula::new_true(pg_id);

        // Var representing the current event
        let current_event = self.cs.new_var(pg_id, VarType::Integer)?;
        // Implement internal queue
        let int_queue = self.cs.new_channel(VarType::Integer, None);
        let int_dequeue =
            self.cs
                .new_communication(pg_id, int_queue, crate::Message::Receive(current_event))?;
        // Implement external queue
        let ext_queue = self.cs.new_channel(VarType::Integer, None);
        let ext_dequeue =
            self.cs
                .new_communication(pg_id, ext_queue, crate::Message::Receive(current_event))?;
        // action representing checking the next transition
        let next_transition = self.cs.new_action(pg_id)?;

        for (state_name, state) in fsm.states.iter() {
            // Each state is modeled by multiple locations connected by transitions
            // starting location and where transitions with NULL event activate
            let state_loc = if let Some(start_loc) = states.get(state_name) {
                *start_loc
            } else {
                let start_loc = self.cs.new_location(pg_id)?;
                states.insert(state_name.to_owned(), start_loc);
                start_loc
            };
            let mut null_loc = state_loc;

            // location where transitions with named event from internal queue activate
            let int_queue_loc = self.cs.new_location(pg_id)?;
            // location where transitions with named event from external queue activate
            let ext_queue_loc = self.cs.new_location(pg_id)?;
            let mut named_trans = self.cs.new_location(pg_id)?;
            // dequeue a new internal event and search for first active named transition
            self.cs.add_transition(
                pg_id,
                int_queue_loc,
                int_dequeue,
                named_trans,
                true_cond.to_owned(),
            )?;
            // check if internal queue is empty;
            // if so, move to external queue
            // notice that one and only one of int_dequeue and empty_int_queue can be executed at a given time
            let empty_int_queue =
                self.cs
                    .new_communication(pg_id, int_queue, crate::Message::ProbeEmptyQueue)?;
            self.cs.add_transition(
                pg_id,
                int_queue_loc,
                empty_int_queue,
                ext_queue_loc,
                true_cond.to_owned(),
            )?;
            // dequeue a new external event and search for first active named transition
            self.cs.add_transition(
                pg_id,
                ext_queue_loc,
                ext_dequeue,
                named_trans,
                true_cond.to_owned(),
            )?;

            for transition in state.transitions.iter() {
                // get or create the location corresponding to the target state
                let target_loc = if let Some(target_loc) = states.get(&transition.target) {
                    *target_loc
                } else {
                    let target_loc = self.cs.new_location(pg_id)?;
                    states.insert(transition.target.to_owned(), target_loc);
                    target_loc
                };

                // action correponding to execute the transition
                let action = self.cs.new_action(pg_id)?;
                // location corresponding to the original state after verifying the transition is not active
                let next_trans_loc = self.cs.new_location(pg_id)?;
                // TODO: implement guards
                let guard = CsFormula::new_true(pg_id);
                let not_guard = CsFormula::negation(guard.to_owned());
                // TODO: add effects

                if let Some(event) = &transition.event {
                    // named event transition
                    let event_id = if let Some(&event_id) = self.events.get(event) {
                        event_id
                    } else {
                        let event_id = self.events.len() as Integer;
                        self.events.insert(event.to_owned(), event_id);
                        event_id
                    };
                    // check if the current event (internal or external) corresponds to the event activating the transition
                    let guard = CsFormula::and(
                        guard,
                        CsFormula::eq(
                            CsIntExpr::new_var(current_event),
                            CsIntExpr::new_const(pg_id, event_id),
                        )?,
                    )?;
                    let not_guard = CsFormula::negation(guard.to_owned());
                    self.cs.add_transition(
                        pg_id,
                        named_trans,
                        action,
                        target_loc,
                        guard.to_owned(),
                    )?;
                    self.cs.add_transition(
                        pg_id,
                        named_trans,
                        next_transition,
                        next_trans_loc,
                        not_guard,
                    )?;
                    named_trans = next_trans_loc;
                } else {
                    // NULL (unnamed) event transition
                    self.cs.add_transition(
                        pg_id,
                        null_loc,
                        action,
                        target_loc,
                        guard.to_owned(),
                    )?;
                    self.cs
                        .add_transition(pg_id, null_loc, action, next_trans_loc, not_guard)?;
                    null_loc = next_trans_loc;
                }
            }

            // connect NULL events with named events
            // by transitioning from last "NUll" location to dequeuing event location
            self.cs.add_transition(
                pg_id,
                null_loc,
                next_transition,
                int_queue_loc,
                true_cond.to_owned(),
            )?;
            // return to dequeue a new (internal or external) event
            self.cs.add_transition(
                pg_id,
                named_trans,
                next_transition,
                int_queue_loc,
                true_cond.to_owned(),
            )?;
        }
        Ok(())
    }

    fn build(self) -> CsModel {
        let skill_names = self
            .skill_ids
            .iter()
            .map(|(name, id)| (*id, name.to_owned()))
            .collect();
        let component_names = self
            .component_ids
            .iter()
            .map(|(name, id)| (*id, name.to_owned()))
            .collect();
        CsModel {
            cs: self.cs.build(),
            skill_ids: self.skill_ids,
            skill_names,
            component_ids: self.component_ids,
            component_names,
        }
    }
}
