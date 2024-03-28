use std::{collections::HashMap, env::current_dir};

use anyhow::Ok;
use log::info;

use crate::{
    parser::*, Channel, ChannelSystem, ChannelSystemBuilder, CsExpr, CsFormula, CsIntExpr, Integer,
    PgId, VarType,
};

#[derive(Debug)]
pub struct CsModel {
    cs: ChannelSystem,
    fsm_names: HashMap<PgId, String>,
    // skill_ids: HashMap<String, PgId>,
    // skill_names: HashMap<PgId, String>,
    // component_ids: HashMap<String, PgId>,
    // component_names: HashMap<PgId, String>,
}

#[derive(Debug)]
pub struct Sc2CsVisitor {
    cs: ChannelSystemBuilder,
    fsm_ids: HashMap<String, PgId>,
    // skill_ids: HashMap<String, PgId>,
    // component_ids: HashMap<String, PgId>,
    external_queues: HashMap<String, Channel>,
    events: HashMap<String, Integer>,
}

impl Sc2CsVisitor {
    pub fn visit(parser: Parser) -> anyhow::Result<CsModel> {
        let mut model = Sc2CsVisitor {
            cs: ChannelSystemBuilder::new(),
            // skill_ids: HashMap::new(),
            // component_ids: HashMap::new(),
            fsm_ids: HashMap::new(),
            external_queues: HashMap::new(),
            events: HashMap::new(),
        };

        info!("Visit skill list");
        for (id, declaration) in parser.skill_list.iter() {
            if let MoC::Fsm(fsm) = &declaration.moc {
                info!("Visit skill {id}");
                model.build_fsm(fsm)?;
            }
        }

        info!("Visit component list");
        for (id, declaration) in parser.component_list.iter() {
            if let MoC::Fsm(fsm) = &declaration.moc {
                info!("Visit component {id}");
                model.build_fsm(fsm)?;
            }
        }

        let model = model.build();

        Ok(model)
    }

    // TODO: Optimize CS by removing unnecessary states
    fn build_fsm(&mut self, fsm: &Fsm) -> anyhow::Result<()> {
        // Initialize fsm.
        let pg_id = self.fsm_ids.get(&fsm.id).cloned().unwrap_or_else(|| {
            let pg_id = self.cs.new_program_graph();
            self.fsm_ids.insert(fsm.id.to_owned(), pg_id);
            pg_id
        });
        // Map fsm's state ids to corresponding CS's locations.
        let mut states = HashMap::new();
        let initial = self.cs.initial_location(pg_id)?;
        // Conventionally, the entry-point for a state is a location associated to the id of the state.
        // In particular, the id of the initial state of the fsm has to correspond to the initial location of the program graph.
        states.insert(fsm.initial.to_owned(), initial);
        let true_cond = CsFormula::new_true(pg_id);

        // Var representing the current event
        let current_event = self.cs.new_var(pg_id, VarType::Integer)?;
        // Implement internal queue
        let int_queue = self.cs.new_channel(VarType::Integer, None);
        let dequeue_int =
            self.cs
                .new_communication(pg_id, int_queue, crate::Message::Receive(current_event))?;
        // Implement external queue
        let ext_queue = self
            .external_queues
            .get(&fsm.id)
            .cloned()
            .unwrap_or_else(|| self.cs.new_channel(VarType::Integer, None));
        let dequeue_ext =
            self.cs
                .new_communication(pg_id, ext_queue, crate::Message::Receive(current_event))?;
        // action representing checking the next transition
        let next_transition = self.cs.new_action(pg_id)?;

        // Consider each of the fsm's states
        for (state_name, state) in fsm.states.iter() {
            // Each state is modeled by multiple locations connected by transitions
            // Starting location
            let start_loc = if let Some(start_loc) = states.get(state_name) {
                *start_loc
            } else {
                let start_loc = self.cs.new_location(pg_id)?;
                states.insert(state_name.to_owned(), start_loc);
                start_loc
            };
            // Execute the state's `onentry` executable content
            let mut onentry_loc = start_loc;
            for executable in state.on_entry.iter() {
                match executable {
                    Executable::Raise { event } => {
                        let event_idx = self.events.get(event).cloned().unwrap_or_else(|| {
                            let event_idx = self.events.len() as Integer;
                            self.events.insert(event.to_owned(), event_idx);
                            event_idx
                        });
                        let raise = self.cs.new_communication(
                            pg_id,
                            int_queue,
                            crate::Message::Send(CsExpr::from_expr(CsIntExpr::new_const(
                                pg_id, event_idx,
                            ))),
                        )?;
                        let next_loc = self.cs.new_location(pg_id)?;
                        // queue the internal event
                        self.cs.add_transition(
                            pg_id,
                            onentry_loc,
                            raise,
                            next_loc,
                            true_cond.to_owned(),
                        )?;
                        onentry_loc = next_loc;
                    }
                    Executable::Send { event, target } => {
                        let target_id = self.fsm_ids.get(target).cloned().unwrap_or_else(|| {
                            let pg_id = self.cs.new_program_graph();
                            self.fsm_ids.insert(target.to_owned(), pg_id);
                            pg_id
                        });
                        let event_id = self.events.get(event).cloned().unwrap_or_else(|| {
                            let event_id = self.events.len() as Integer;
                            self.events.insert(event.to_owned(), event_id);
                            event_id
                        });
                        let target_ext_queue = self
                            .external_queues
                            .get(target)
                            .cloned()
                            .unwrap_or_else(|| self.cs.new_channel(VarType::Integer, None));
                        let send = self.cs.new_communication(
                            pg_id,
                            target_ext_queue,
                            crate::Message::Send(CsExpr::from_expr(CsIntExpr::new_const(
                                target_id, event_id,
                            ))),
                        )?;
                        let next_loc = self.cs.new_location(pg_id)?;
                        // queue the internal event
                        self.cs.add_transition(
                            pg_id,
                            onentry_loc,
                            send,
                            next_loc,
                            true_cond.to_owned(),
                        )?;
                        onentry_loc = next_loc;
                    }
                }
            }

            // location where eventless transitions activate
            let mut null_trans = onentry_loc;
            // location where transitions with named event from internal queue activate
            let int_queue_loc = self.cs.new_location(pg_id)?;
            // location where transitions with named event from external queue activate
            let ext_queue_loc = self.cs.new_location(pg_id)?;
            let mut named_trans = self.cs.new_location(pg_id)?;
            // dequeue a new internal event and search for first active named transition
            self.cs.add_transition(
                pg_id,
                int_queue_loc,
                dequeue_int,
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
                dequeue_ext,
                named_trans,
                true_cond.to_owned(),
            )?;

            // consider each of the state's transitions
            for transition in state.transitions.iter() {
                // get or create the location corresponding to the target state
                let target_loc = if let Some(target_loc) = states.get(&transition.target) {
                    *target_loc
                } else {
                    let target_loc = self.cs.new_location(pg_id)?;
                    states.insert(transition.target.to_owned(), target_loc);
                    target_loc
                };

                // action correponding to executing the transition
                let exec_transition = self.cs.new_action(pg_id)?;
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
                        exec_transition,
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
                        null_trans,
                        exec_transition,
                        target_loc,
                        guard.to_owned(),
                    )?;
                    self.cs.add_transition(
                        pg_id,
                        null_trans,
                        next_transition,
                        next_trans_loc,
                        not_guard,
                    )?;
                    null_trans = next_trans_loc;
                }
            }

            // connect NULL events with named events
            // by transitioning from last "NUll" location to dequeuing event location
            self.cs.add_transition(
                pg_id,
                null_trans,
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
        let fsm_names = self
            .fsm_ids
            .iter()
            .map(|(name, id)| (*id, name.to_owned()))
            .collect();
        // let skill_names = self
        //     .skill_ids
        //     .iter()
        //     .map(|(name, id)| (*id, name.to_owned()))
        //     .collect();
        // let component_names = self
        //     .component_ids
        //     .iter()
        //     .map(|(name, id)| (*id, name.to_owned()))
        //     .collect();
        CsModel {
            cs: self.cs.build(),
            fsm_names,
            // skill_ids: self.skill_ids,
            // skill_names,
            // component_ids: self.component_ids,
            // component_names,
        }
    }
}
