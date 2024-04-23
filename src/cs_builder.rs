use std::{collections::HashMap, ops::Index};

use crate::{parser::vocabulary::*, CsAction};
use anyhow::Ok;
use log::{info, trace};

use crate::{
    parser::*, Channel, ChannelSystem, ChannelSystemBuilder, CsExpr, CsFormula, CsIntExpr,
    CsLocation, CsVar, Integer, Message, PgId, VarType,
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
    // Each State Chart has an associated Program Graph,
    // and an arbitrary, progressive index
    moc_ids: HashMap<String, usize>,
    moc_pgid: Vec<PgId>,
    // skill_ids: HashMap<String, PgId>,
    // component_ids: HashMap<String, PgId>,
    // Each State Chart has an associated external event queue.
    external_queues: HashMap<String, Channel>,
    // Each event is associated to a unique global index.
    events: HashMap<String, Integer>,
    // For each State Chart, each variable is associated to an index.
    vars: HashMap<(PgId, String), CsVar>,
    // Events carrying parameters have dedicated channels for them,
    // one for each senderStateChart+sentEvent+paramName that is needed
    parameters: HashMap<(PgId, Integer, String), Channel>,
}

impl Sc2CsVisitor {
    pub fn visit(parser: Parser) -> anyhow::Result<CsModel> {
        let mut model = Sc2CsVisitor {
            cs: ChannelSystemBuilder::new(),
            // skill_ids: HashMap::new(),
            // component_ids: HashMap::new(),
            moc_ids: HashMap::new(),
            moc_pgid: Vec::new(),
            external_queues: HashMap::new(),
            events: HashMap::new(),
            parameters: HashMap::new(),
            vars: HashMap::new(),
        };

        // info!("build tick generator");
        // model.build_tick_generator(&parser.task_plan)?;

        info!("Visit skill list");
        for (id, declaration) in parser.skill_list.iter() {
            info!("Visit skill {id}");
            match &declaration.moc {
                MoC::Fsm(fsm) => model.build_fsm(fsm)?,
                MoC::Bt(bt) => model.build_bt(bt)?,
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

    fn get_event_idx(&mut self, id: &str) -> Integer {
        self.events.get(id).cloned().unwrap_or_else(|| {
            let idx = self.events.len() as Integer;
            self.events.insert(id.to_owned(), idx);
            idx
        })
    }

    fn get_moc_pgid(&mut self, id: &str) -> PgId {
        self.moc_ids
            .get(id)
            .map(|idx| self.moc_pgid[*idx])
            .unwrap_or_else(|| {
                let idx = self.moc_pgid.len();
                self.moc_ids.insert(id.to_owned(), idx);
                let pg_id = self.cs.new_program_graph();
                self.moc_pgid.push(pg_id);
                pg_id
            })
    }

    fn get_external_queue(&mut self, id: &str) -> Channel {
        self.external_queues.get(id).cloned().unwrap_or_else(|| {
            let external_queue = self.cs.new_channel(
                VarType::Integer,
                // VarType::Product(vec![VarType::Integer, VarType::Integer]),
                None,
            );
            self.external_queues.insert(id.to_owned(), external_queue);
            external_queue
        })
    }

    /// Builds a simple Program Graph that sends TickCall events to the task plan,
    /// and receives TickResponse values,
    /// as long as the response is Running.
    /// When it receives Success or Failure, it stops.
    ///
    /// Informal description of TickGenerator PG:
    ///
    // fn build_tick_generator(&mut self, task_plan: &str) -> anyhow::Result<()> {
    //     let pg_id = self.cs.new_program_graph();
    //     let send_tick_loc = self.cs.new_location(pg_id)?;
    //     // Create tick event, if it does not exist already.
    //     let tick_call_event = self.get_event_idx(TICK_CALL);
    //     // Build external queue for task_plan, if it does not exist already.
    //     let task_plan_external_queue = self.get_external_queue(task_plan);
    //     // Send a tick to the task_plan.
    //     let tick_task_plan = self.cs.new_communication(
    //         pg_id,
    //         task_plan_external_queue,
    //         Message::Send(CsExpr::from_expr(CsIntExpr::new_const(
    //             pg_id,
    //             tick_call_event,
    //         ))),
    //     )?;
    //     let wait_response_loc = self.cs.new_location(pg_id)?;
    //     let tick_response = self.cs.new_var(pg_id, VarType::Integer)?;
    //     // While system is Running, tick task plan.
    //     self.cs.add_transition(
    //         pg_id,
    //         send_tick_loc,
    //         tick_task_plan,
    //         wait_response_loc,
    //         CsFormula::eq(
    //             CsIntExpr::new_var(tick_response),
    //             CsIntExpr::new_const(pg_id, 0),
    //         )?,
    //     )?;

    //     // Implement channel receiving the tick response.
    //     // TODO: capacity should be Some(0), i.e., handshake.
    //     let receive_tick_response_chn = self.get_external_queue(TICK_GENERATOR);
    //     let receive_response = self.cs.new_communication(
    //         pg_id,
    //         receive_tick_response_chn,
    //         Message::Receive(tick_response),
    //     )?;
    //     // Now the tick generator waits for response on its own channel
    //     self.cs.add_transition(
    //         pg_id,
    //         wait_response_loc,
    //         receive_response,
    //         send_tick_loc,
    //         CsFormula::new_true(pg_id),
    //     )?;
    //     Ok(())
    // }

    fn build_bt(&mut self, bt: &Bt) -> anyhow::Result<()> {
        trace!("build bt {}", bt.id);
        // Initialize bt.
        // let pg_id = self.get_moc_pgid(&bt.id);
        // // Implement channel receiving the tick event.
        // // TODO: capacity should be Some(0), i.e., handshake.
        // let tick_receive_chn = self.get_external_queue(&bt.id);
        // // The tick signal carries no data, so it is of type unit.
        // let tick_event = self.cs.new_var(pg_id, VarType::Integer)?;
        // let receive_tick =
        //     self.cs
        //         .new_communication(pg_id, tick_receive_chn, Message::Receive(tick_event))?;
        // // TODO: implement tick_response with enum type.
        // // tick_response, manually implemented:
        // // -1 = Failure
        // //  0 = Running (default value)
        // //  1 = Success
        // let tick_response = self.cs.new_var(pg_id, VarType::Integer)?;
        // // Implement channel receiving the tick response.
        // // TODO: capacity should be Some(0), i.e., handshake.
        // let tick_response_chn = self.get_external_queue(&(bt.id.to_owned() + "Response"));
        // let receive_response =
        //     self.cs
        //         .new_communication(pg_id, tick_response_chn, Message::Receive(tick_response))?;
        // let root = self.cs.new_location(pg_id)?;
        // let (tick_node_in, tick_node_out) =
        //     self.build_bt_node(pg_id, tick_response, receive_response, &bt.root)?;
        // // Receiving tick on `tick_chn` transitions out of root into its child node.
        // self.cs.add_transition(
        //     pg_id,
        //     root,
        //     receive_tick,
        //     tick_node_in,
        //     CsFormula::new_true(pg_id),
        // )?;
        Ok(())
    }

    fn build_bt_node(
        &mut self,
        pg_id: PgId,
        tick_response: CsVar,
        receive_response: CsAction,
        node: &BtNode,
    ) -> anyhow::Result<(CsLocation, CsLocation)> {
        let tick_in = self.cs.new_location(pg_id)?;
        let tick_out = self.cs.new_location(pg_id)?;

        match node {
            BtNode::RSeq(branches) => {}
            BtNode::RFbk(branches) => {}
            BtNode::MSeq(branches) => {}
            BtNode::MFbk(branches) => {}
            BtNode::Invr(branch) => {
                // Recursively build branch BT and get its PoE location.
                let (tick_child_in, tick_child_out) =
                    self.build_bt_node(pg_id, tick_response, receive_response, branch)?;
                // Propagate the tick to the branch.
                let tick = self.cs.new_action(pg_id)?;
                self.cs.add_transition(
                    pg_id,
                    tick_in,
                    tick,
                    tick_child_in,
                    CsFormula::new_true(pg_id),
                )?;
                // Inverting the tick_response:
                // -1 ->  1
                //  0 ->  0
                //  1 -> -1
                let invert = self.cs.new_action(pg_id)?;
                self.cs.add_effect(
                    pg_id,
                    invert,
                    tick_response,
                    CsExpr::from_expr(CsIntExpr::opposite(CsIntExpr::new_var(tick_response))),
                )?;
                self.cs.add_transition(
                    pg_id,
                    tick_child_out,
                    invert,
                    tick_out,
                    CsFormula::new_true(pg_id),
                )?;
            }
            BtNode::LAct(id) | BtNode::LCnd(id) => {
                // Build external queue for skill, if it does not exist already.
                let external_queue = self.external_queues.get(id).cloned().unwrap_or_else(|| {
                    let external_queue = self.cs.new_channel(VarType::Integer, None);
                    self.external_queues.insert(id.to_owned(), external_queue);
                    external_queue
                });
                // Create tick event, if it does not exist already.
                let tick_call_event = self.get_event_idx(TICK_CALL);
                // Send a tickCall event to the skill.
                let tick_skill = self.cs.new_communication(
                    pg_id,
                    external_queue,
                    Message::Send(CsExpr::from_expr(CsIntExpr::new_const(
                        pg_id,
                        tick_call_event,
                    ))),
                )?;
                let wait_response_loc = self.cs.new_location(pg_id)?;
                self.cs.add_transition(
                    pg_id,
                    tick_in,
                    tick_skill,
                    wait_response_loc,
                    CsFormula::new_true(pg_id),
                )?;
                // Now leaf waits for response on its own channel
                self.cs.add_transition(
                    pg_id,
                    wait_response_loc,
                    receive_response,
                    tick_out,
                    CsFormula::new_true(pg_id),
                )?;
            }
        }

        Ok((tick_in, tick_out))
    }

    // TODO: Optimize CS by removing unnecessary states
    fn build_fsm(&mut self, fsm: &Fsm) -> anyhow::Result<()> {
        trace!("build fsm {}", fsm.id);
        // Initialize fsm.
        let pg_id = self.get_moc_pgid(&fsm.id);
        let pg_idx = *self.moc_ids.get(&fsm.id).expect("should exist") as Integer;
        // Map fsm's state ids to corresponding CS's locations.
        let mut states = HashMap::new();
        let initial = self.cs.initial_location(pg_id)?;
        // Conventionally, the entry-point for a state is a location associated to the id of the state.
        // In particular, the id of the initial state of the fsm has to correspond to the initial location of the program graph.
        states.insert(fsm.initial.to_owned(), initial);
        // Var representing the current event
        let current_event = self.cs.new_var(pg_id, VarType::Integer)?;
        // Variable that will store origin of last processed event.
        let origin = self.cs.new_var(pg_id, VarType::Integer)?;
        // Implement internal queue
        let int_queue = self.cs.new_channel(VarType::Integer, None);
        let dequeue_int =
            self.cs
                .new_communication(pg_id, int_queue, Message::Receive(current_event))?;
        let dequeue_int_origin = self.cs.new_action(pg_id)?;
        self.cs.add_effect(
            pg_id,
            dequeue_int_origin,
            origin,
            CsExpr::from_expr(CsIntExpr::new_const(pg_id, pg_idx)),
        )?;
        // Implement external queue
        let ext_queue = self.get_external_queue(&fsm.id);
        let dequeue_ext =
            self.cs
                .new_communication(pg_id, ext_queue, Message::Receive(current_event))?;
        // We also need to receive the event origin info
        let ext_queue_origin = self.get_external_queue(&(fsm.id.to_owned() + "Origin"));
        let dequeue_ext_origin =
            self.cs
                .new_communication(pg_id, ext_queue_origin, Message::Receive(origin))?;
        // Action representing checking the next transition
        let next_transition = self.cs.new_action(pg_id)?;

        trace!("fsm initialization done");

        // Consider each of the fsm's states
        for (_, state) in fsm.states.iter() {
            trace!("build state {}", state.id);
            // Each state is modeled by multiple locations connected by transitions
            // A starting location is used as a point-of-entry to the execution of the state.
            let start_loc = if let Some(start_loc) = states.get(&state.id) {
                *start_loc
            } else {
                let start_loc = self.cs.new_location(pg_id)?;
                states.insert(state.id.to_owned(), start_loc);
                start_loc
            };
            // Execute the state's `onentry` executable content
            let mut onentry_loc = start_loc;
            for executable in state.on_entry.iter() {
                // Each executable content attaches suitable transitions to the point-of-entry location
                // and returns the target of such transitions as updated point-of-entry location.
                onentry_loc =
                    self.add_executable(executable, pg_id, pg_idx, int_queue, onentry_loc)?;
            }

            // Location where eventless/NULL transitions activate
            let mut null_trans = onentry_loc;
            // Location where internal events are dequeued
            let int_queue_loc = self.cs.new_location(pg_id)?;
            // Location where the origin of internal events is set as own.
            let int_queue_origin_loc = self.cs.new_location(pg_id)?;
            // Location where external events are dequeued
            let ext_queue_loc = self.cs.new_location(pg_id)?;
            // Location where the origin of external events is dequeued
            let ext_queue_origin_loc = self.cs.new_location(pg_id)?;
            // Location where eventful transitions activate
            let mut eventful_trans = self.cs.new_location(pg_id)?;
            // Transition dequeueing a new internal event and searching for first active eventful transition
            self.cs.add_transition(
                pg_id,
                int_queue_loc,
                dequeue_int,
                int_queue_origin_loc,
                CsFormula::new_true(pg_id),
            )?;
            // Transition dequeueing a new internal event and searching for first active eventful transition
            self.cs.add_transition(
                pg_id,
                int_queue_origin_loc,
                dequeue_int_origin,
                eventful_trans,
                CsFormula::new_true(pg_id),
            )?;
            // Action denoting checking if internal queue is empty;
            // if so, move to external queue.
            // Notice that one and only one of `int_dequeue` and `empty_int_queue` can be executed at a given time.
            let empty_int_queue =
                self.cs
                    .new_communication(pg_id, int_queue, crate::Message::ProbeEmptyQueue)?;
            self.cs.add_transition(
                pg_id,
                int_queue_loc,
                empty_int_queue,
                ext_queue_loc,
                CsFormula::new_true(pg_id),
            )?;
            // Dequeue a new external event and search for first active named transition.
            self.cs.add_transition(
                pg_id,
                ext_queue_loc,
                dequeue_ext,
                ext_queue_origin_loc,
                CsFormula::new_true(pg_id),
            )?;
            // Dequeue the origin of the corresponding external event.
            self.cs.add_transition(
                pg_id,
                ext_queue_origin_loc,
                dequeue_ext_origin,
                eventful_trans,
                CsFormula::new_true(pg_id),
            )?;

            // Consider each of the state's transitions.
            for transition in state.transitions.iter() {
                trace!("build transition {transition:#?}");
                // Get or create the location corresponding to the target state.
                let target_loc = states.get(&transition.target).cloned().unwrap_or_else(|| {
                    let target_loc = self.cs.new_location(pg_id).expect("pg_id should exist");
                    states.insert(transition.target.to_owned(), target_loc);
                    target_loc
                });

                // Location corresponding to checking if the transition is active.
                // Has to be defined depending on the type of transition.
                let check_trans_loc;
                // Action correponding to executing the transition.
                let exec_transition = self.cs.new_action(pg_id)?;
                // Location corresponding to verifying the transition is not active and moving to next one.
                let next_trans_loc = self.cs.new_location(pg_id)?;
                // Guard for transition.
                // Has to be defined depending on the type of transition, etc...
                let guard;
                // TODO: implement guards
                // TODO: add effects

                // Proceed on whether the transition is eventless or activated by event.
                if let Some(event) = &transition.event {
                    // Create tick event, if it does not exist already.
                    let event_idx = self.get_event_idx(event);
                    // Check if the current event (internal or external) corresponds to the event activating the transition.
                    guard = CsFormula::eq(
                        CsIntExpr::new_var(current_event),
                        CsIntExpr::new_const(pg_id, event_idx),
                    )?;
                    // Check this transition after the other eventful transitions.
                    check_trans_loc = eventful_trans;
                    // Move location of next eventful transitions to a new location.
                    eventful_trans = next_trans_loc;
                } else {
                    // // NULL (unnamed) event transition
                    // No event needs to happen in order to trigger this transition.
                    guard = CsFormula::new_true(pg_id);
                    // Check this transition after the other eventless transitions.
                    check_trans_loc = null_trans;
                    // Move location of next eventless transitions to a new location.
                    null_trans = next_trans_loc;
                }

                // If transition is active, execute the relevant executable content and then the transition to the target.
                {
                    let mut exec_trans_loc = self.cs.new_location(pg_id)?;
                    self.cs.add_transition(
                        pg_id,
                        check_trans_loc,
                        exec_transition,
                        exec_trans_loc,
                        guard.to_owned(),
                    )?;
                    // First execute the executable content of the state's `on_exit` tag,
                    // then that of the `transition` tag.
                    for exec in state.on_exit.iter().chain(transition.effects.iter()) {
                        exec_trans_loc =
                            self.add_executable(exec, pg_id, pg_idx, int_queue, exec_trans_loc)?;
                    }
                    // Transitioning to the target state/location.
                    // At this point, the transition cannot be stopped so there can be no guard.
                    self.cs.add_transition(
                        pg_id,
                        exec_trans_loc,
                        exec_transition,
                        target_loc,
                        CsFormula::new_true(pg_id),
                    )?;
                }
                // If the current transition is not active, move on to check the next one.
                let not_guard = CsFormula::negation(guard.to_owned());
                self.cs.add_transition(
                    pg_id,
                    check_trans_loc,
                    next_transition,
                    next_trans_loc,
                    not_guard,
                )?;
            }

            // Connect NULL events with named events
            // by transitioning from last "NUll" location to dequeuing event location.
            self.cs.add_transition(
                pg_id,
                null_trans,
                next_transition,
                int_queue_loc,
                CsFormula::new_true(pg_id),
            )?;
            // Return to dequeue a new (internal or external) event.
            self.cs.add_transition(
                pg_id,
                eventful_trans,
                next_transition,
                int_queue_loc,
                CsFormula::new_true(pg_id),
            )?;
        }
        Ok(())
    }

    fn add_executable(
        &mut self,
        executable: &Executable,
        pg_id: PgId,
        pg_idx: Integer,
        int_queue: Channel,
        loc: CsLocation,
    ) -> Result<CsLocation, anyhow::Error> {
        match executable {
            Executable::Raise { event } => {
                // Create event, if it does not exist already.
                let event_idx = self.get_event_idx(event);
                let raise = self.cs.new_communication(
                    pg_id,
                    int_queue,
                    crate::Message::Send(CsExpr::from_expr(CsIntExpr::new_const(pg_id, event_idx))),
                )?;
                let next_loc = self.cs.new_location(pg_id)?;
                // queue the internal event
                self.cs
                    .add_transition(pg_id, loc, raise, next_loc, CsFormula::new_true(pg_id))?;
                Ok(next_loc)
            }
            Executable::Send {
                event,
                target,
                params,
            } => {
                let target_id = self.get_moc_pgid(target);
                // Create event, if it does not exist already.
                let event_idx = self.get_event_idx(event);
                let target_ext_queue = self.get_external_queue(target);
                let send_event = self.cs.new_communication(
                    pg_id,
                    target_ext_queue,
                    crate::Message::Send(CsExpr::from_expr(CsIntExpr::new_const(pg_id, event_idx))),
                )?;
                let target_ext_queue_origin =
                    self.get_external_queue(&(target.to_owned() + "Origin"));
                let send_origin = self.cs.new_communication(
                    pg_id,
                    target_ext_queue_origin,
                    crate::Message::Send(CsExpr::from_expr(CsIntExpr::new_const(pg_id, pg_idx))),
                )?;

                // // Pass parameters.
                // for param in params {
                //     // Get associated variable.
                //     let var = self.vars.get_mut(&(pg_id, param.location.to_owned()));
                //     // Get or create suitable channel.
                // }

                // Send event and event origin before moving on to next location.
                let event_loc = self.cs.new_location(pg_id)?;
                self.cs.add_transition(
                    pg_id,
                    loc,
                    send_event,
                    event_loc,
                    CsFormula::new_true(pg_id),
                )?;
                let next_loc = self.cs.new_location(pg_id)?;
                self.cs.add_transition(
                    pg_id,
                    event_loc,
                    send_origin,
                    next_loc,
                    CsFormula::new_true(pg_id),
                )?;
                Ok(next_loc)
            }
        }
    }

    fn build(self) -> CsModel {
        let fsm_names = self
            .moc_ids
            .iter()
            .map(|(name, id)| (self.moc_pgid[*id], name.to_owned()))
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
