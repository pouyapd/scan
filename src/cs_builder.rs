use std::{any::Any, collections::HashMap, fmt::Debug, str::FromStr};

use crate::{parser::vocabulary::*, CsAction, Val, Var};
use anyhow::{anyhow, Ok};
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
    // Represent OMG types
    scan_types: HashMap<String, VarType>,
    enums: HashMap<(String, String), Integer>,
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
    vars: HashMap<(PgId, String), (CsVar, VarType)>,
    // Events carrying parameters have dedicated channels for them,
    // one for each:
    // - senderStateChart
    // - receiverStateChart
    // - sentEvent
    // - paramName
    // that is needed
    parameters: HashMap<(PgId, PgId, Integer, String), Channel>,
}

impl Sc2CsVisitor {
    pub fn visit(parser: Parser) -> anyhow::Result<CsModel> {
        let mut model = Sc2CsVisitor {
            cs: ChannelSystemBuilder::new(),
            scan_types: HashMap::new(),
            enums: HashMap::new(),
            // skill_ids: HashMap::new(),
            // component_ids: HashMap::new(),
            moc_ids: HashMap::new(),
            moc_pgid: Vec::new(),
            external_queues: HashMap::new(),
            events: HashMap::new(),
            parameters: HashMap::new(),
            vars: HashMap::new(),
        };

        // FIXME: Is there a better way? Const object?
        model
            .scan_types
            .insert(String::from_str("int32").unwrap(), VarType::Integer);
        model
            .scan_types
            .insert(String::from_str("URI").unwrap(), VarType::Integer);
        model
            .scan_types
            .insert(String::from_str("Boolean").unwrap(), VarType::Boolean);

        model.build_types(&parser.types)?;

        info!("Visit process list");
        for (id, declaration) in parser.process_list.iter() {
            info!("Visit process {id}");
            match &declaration.moc {
                MoC::Fsm(fsm) => model.build_fsm(fsm)?,
                MoC::Bt(bt) => model.build_bt(bt)?,
            }
        }

        let model = model.build();

        Ok(model)
    }

    fn build_types(&mut self, omg_types: &OmgTypes) -> anyhow::Result<()> {
        for (name, omg_type) in omg_types.types.iter() {
            let scan_type = match omg_type {
                OmgType::Boolean => VarType::Boolean,
                OmgType::Int32 => VarType::Integer,
                OmgType::Structure() => todo!(),
                OmgType::Enumeration(labels) => {
                    for (idx, label) in labels.iter().enumerate() {
                        self.enums
                            .insert((name.to_owned(), label.to_owned()), idx as Integer);
                    }
                    VarType::Integer
                }
            };
            self.scan_types.insert(name.to_owned(), scan_type);
        }
        Ok(())
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

    fn get_origin_channel(&mut self, pg_id: PgId) -> Channel {
        let ch_name = format!("BLDR:{pg_id:?}:origin");
        self.get_external_queue(&ch_name)
    }

    // fn build_bt(&mut self, bt: &Bt) -> anyhow::Result<()> {
    //     trace!("build bt {}", bt.id);
    //     // Initialize bt.
    //     let pg_id = self.get_moc_pgid(&bt.id);
    //     // Implement channel receiving the tick event.
    //     // TODO: the external queue origin channel is never emptied.
    //     // TODO: capacity should be Some(0), i.e., handshake.
    //     let tick_receive_chn = self.get_external_queue(&bt.id);
    //     // The tick signal carries no data, so it is of type unit.
    //     let tick_event = self.cs.new_var(pg_id, VarType::Integer)?;
    //     let receive_tick =
    //         self.cs
    //             .new_communication(pg_id, tick_receive_chn, Message::Receive(tick_event))?;
    //     // TODO: implement tick_response with enum type.
    //     // tick_response, manually implemented:
    //     // -1 = Failure
    //     //  0 = Running (default value)
    //     //  1 = Success
    //     let tick_response = self.cs.new_var(pg_id, VarType::Integer)?;
    //     // Implement channel receiving the tick response.
    //     // TODO: capacity should be Some(0), i.e., handshake.
    //     let tick_response_chn = self.get_external_queue(&(bt.id.to_owned() + "Response"));
    //     let receive_response =
    //         self.cs
    //             .new_communication(pg_id, tick_response_chn, Message::Receive(tick_response))?;
    //     let root = self.cs.new_location(pg_id)?;
    //     let (tick_node_in, tick_node_out) =
    //         self.build_bt_node(pg_id, tick_response, receive_response, &bt.root)?;
    //     // Receiving tick on `tick_chn` transitions out of root into its child node.
    //     self.cs.add_transition(
    //         pg_id,
    //         root,
    //         receive_tick,
    //         tick_node_in,
    //         CsFormula::new_true(pg_id),
    //     )?;
    //     Ok(())
    // }

    fn build_bt(&mut self, bt: &Bt) -> anyhow::Result<()> {
        trace!("build bt {}", bt.id);
        // Initialize bt.
        let pg_id = self.get_moc_pgid(&bt.id);
        let loc_tick = self.cs.initial_location(pg_id)?;
        let loc_success = self.cs.new_location(pg_id)?;
        let loc_running = self.cs.new_location(pg_id)?;
        let loc_failure = self.cs.new_location(pg_id)?;
        let loc_halt = self.cs.new_location(pg_id)?;
        let loc_ack = self.cs.new_location(pg_id)?;
        let step = self.cs.new_action(pg_id)?;
        self.cs.add_transition(
            pg_id,
            loc_running,
            step,
            loc_tick,
            CsFormula::new_true(pg_id),
        )?;
        let tick_response_chn = self.get_external_queue(&bt.id);
        let tick_response = self.cs.new_var(pg_id, VarType::Integer)?;
        let receive_response =
            self.cs
                .new_communication(pg_id, tick_response_chn, Message::Receive(tick_response))?;
        // self.build_bt_node(
        //     pg_id,
        //     loc_tick,
        //     loc_success,
        //     loc_running,
        //     loc_failure,
        //     loc_halt,
        //     loc_ack,
        //     step,
        //     tick_response,
        //     receive_response,
        //     &bt.root,
        // )?;
        Ok(())
    }

    /// Recursively build a BT node by associating each possible state of the node to a location:
    /// - *_tick: the node has been sent a tick by its parent node
    /// - *_success: the node has returned with state success
    /// - *_running: the node has returned with state running
    /// - *_failure: the node has returned with state failure
    /// - *_halt: the node has been sent an halt signal by the parent node
    /// - *_halt_success: the node has been sent an halt signal by its parent node after a previous node succeeded (in reactive nodes)
    /// - *_halt_failure: the node has been sent an halt signal by its parent node after a previous node failed (in reactive nodes)
    /// - *_ack: the node has returned an ack signal
    /// - *_ack_success: the node has returned an ack signal after a previous node succeeded (in reactive nodes)
    /// - *_ack_failure: the node has returned an ack signal after a previous node failed (in reactive nodes)
    /// Moreover, we consider the following nodes:
    /// - pt_*: parent
    /// - loc_*: current note (loc=location)
    /// - branch_*: branch/child
    fn build_bt_node(
        &mut self,
        pg_id: PgId,
        pt_tick: CsLocation,
        pt_success: CsLocation,
        pt_running: CsLocation,
        pt_failure: CsLocation,
        pt_halt: CsLocation,
        pt_halt_success: CsLocation,
        pt_halt_failure: CsLocation,
        pt_ack: CsLocation,
        pt_ack_success: CsLocation,
        pt_ack_failure: CsLocation,
        step: CsAction,
        tick_response: CsVar,
        receive_response: CsAction,
        node: &BtNode,
    ) -> anyhow::Result<()> {
        match node {
            BtNode::RSeq(branches) => {
                let mut prev_tick = pt_tick;
                let mut prev_success = pt_tick;
                let mut prev_running = pt_running;
                let mut prev_failure = pt_failure;
                let mut prev_halt = pt_halt;
                let mut prev_halt_success = pt_halt_success;
                let mut prev_halt_failure = pt_halt_failure;
                let mut prev_ack = pt_ack;
                let mut prev_ack_success = pt_ack_success;
                let mut prev_ack_failure = pt_ack_failure;
                for branch in branches {
                    let loc_tick = self.cs.new_location(pg_id)?;
                    let loc_success = self.cs.new_location(pg_id)?;
                    let loc_running = self.cs.new_location(pg_id)?;
                    let loc_failure = self.cs.new_location(pg_id)?;
                    let loc_halt = self.cs.new_location(pg_id)?;
                    let loc_halt_success = self.cs.new_location(pg_id)?;
                    let loc_halt_failure = self.cs.new_location(pg_id)?;
                    let loc_ack = self.cs.new_location(pg_id)?;
                    let loc_ack_success = self.cs.new_location(pg_id)?;
                    let loc_ack_failure = self.cs.new_location(pg_id)?;
                    self.cs.add_transition(
                        pg_id,
                        prev_success,
                        step,
                        loc_tick,
                        CsFormula::new_true(pg_id),
                    )?;
                    self.cs.add_transition(
                        pg_id,
                        loc_running,
                        step,
                        pt_running,
                        CsFormula::new_true(pg_id),
                    )?;
                    self.cs.add_transition(
                        pg_id,
                        prev_failure,
                        step,
                        loc_halt_failure,
                        CsFormula::new_true(pg_id),
                    )?;
                    self.cs.add_transition(
                        pg_id,
                        prev_ack_failure,
                        step,
                        loc_halt_failure,
                        CsFormula::new_true(pg_id),
                    )?;
                    self.cs.add_transition(
                        pg_id,
                        prev_ack,
                        step,
                        loc_halt,
                        CsFormula::new_true(pg_id),
                    )?;
                    self.build_bt_node(
                        pg_id,
                        loc_tick,
                        loc_success,
                        loc_running,
                        loc_failure,
                        loc_halt,
                        loc_halt_success,
                        loc_halt_failure,
                        loc_ack,
                        loc_ack_success,
                        loc_ack_failure,
                        step,
                        tick_response,
                        receive_response,
                        branch,
                    )?;
                    prev_tick = loc_tick;
                    prev_success = loc_success;
                    prev_running = loc_running;
                    prev_failure = loc_failure;
                    prev_halt = loc_halt;
                    prev_halt_success = loc_halt_success;
                    prev_halt_failure = loc_halt_failure;
                    prev_ack = loc_ack;
                    prev_ack_success = loc_ack_success;
                    prev_ack_failure = loc_ack_failure;
                }
                self.cs.add_transition(
                    pg_id,
                    prev_success,
                    step,
                    pt_success,
                    CsFormula::new_true(pg_id),
                )?;
                self.cs.add_transition(
                    pg_id,
                    prev_ack,
                    step,
                    pt_ack,
                    CsFormula::new_true(pg_id),
                )?;
                self.cs.add_transition(
                    pg_id,
                    prev_ack_failure,
                    step,
                    pt_ack_failure,
                    CsFormula::new_true(pg_id),
                )?;
                // self.cs.add_transition(
                //     pg_id,
                //     prev_ack_success,
                //     step,
                //     pt_ack_success,
                //     CsFormula::new_true(pg_id),
                // )?;
            }
            BtNode::RFbk(branches) => {}
            BtNode::MSeq(branches) => {}
            BtNode::MFbk(branches) => {}
            BtNode::Invr(branch) => {
                // Swap success and failure.
                self.build_bt_node(
                    pg_id,
                    pt_tick,
                    pt_failure,
                    pt_running,
                    pt_success,
                    pt_halt,
                    pt_halt_success,
                    pt_halt_failure,
                    pt_ack,
                    pt_ack_success,
                    pt_ack_failure,
                    step,
                    tick_response,
                    receive_response,
                    branch,
                )?;
                // let loc_tick = self.cs.new_location(pg_id)?;
                // let loc_success = self.cs.new_location(pg_id)?;
                // let loc_running = self.cs.new_location(pg_id)?;
                // let loc_failure = self.cs.new_location(pg_id)?;
                // let loc_halt = self.cs.new_location(pg_id)?;
                // let loc_ack = self.cs.new_location(pg_id)?;
                // self.build_bt_node(
                //     pg_id,
                //     loc_tick,
                //     loc_success,
                //     loc_running,
                //     loc_failure,
                //     loc_halt,
                //     loc_ack,
                //     step,
                //     tick_response,
                //     receive_response,
                //     &branch,
                // )?;
                // self.cs.add_transition(
                //     pg_id,
                //     loc_failure,
                //     step,
                //     pt_success,
                //     CsFormula::new_true(pg_id),
                // )?;
                // self.cs.add_transition(
                //     pg_id,
                //     loc_running,
                //     step,
                //     pt_running,
                //     CsFormula::new_true(pg_id),
                // )?;
                // self.cs.add_transition(
                //     pg_id,
                //     loc_success,
                //     step,
                //     pt_failure,
                //     CsFormula::new_true(pg_id),
                // )?;
                // self.cs.add_transition(
                //     pg_id,
                //     pt_halt,
                //     step,
                //     loc_halt,
                //     CsFormula::new_true(pg_id),
                // )?;
                // self.cs
                //     .add_transition(pg_id, loc_ack, step, pt_ack, CsFormula::new_true(pg_id))?;
            }
            BtNode::LAct(id) | BtNode::LCnd(id) => {
                // let ev_tick = self.get_event_idx(&"TICK");
                let ev_tick_success = self.get_event_idx(&"SUCCESS");
                let ev_tick_running = self.get_event_idx(&"RUNNING");
                let ev_tick_failure = self.get_event_idx(&"FAILURE");
                // let ev_halt = self.get_event_idx(&"HALT");
                // let ev_halt_ack = self.get_event_idx(&"ACK");
                let loc_tick = self.cs.new_location(pg_id)?;
                let loc_response = self.cs.new_location(pg_id)?;
                // Build external queue for skill, if it does not exist already.
                let external_queue = self.get_external_queue(id);
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
                self.cs.add_transition(
                    pg_id,
                    pt_tick,
                    tick_skill,
                    loc_tick,
                    CsFormula::new_true(pg_id),
                )?;
                // Now leaf waits for response on its own channel
                self.cs.add_transition(
                    pg_id,
                    loc_tick,
                    receive_response,
                    loc_response,
                    CsFormula::new_true(pg_id),
                )?;
                self.cs.add_transition(
                    pg_id,
                    loc_response,
                    step,
                    pt_success,
                    CsFormula::eq(
                        CsIntExpr::new_const(pg_id, ev_tick_success),
                        CsIntExpr::new_var(tick_response),
                    )?,
                )?;
                self.cs.add_transition(
                    pg_id,
                    loc_response,
                    step,
                    pt_running,
                    CsFormula::eq(
                        CsIntExpr::new_const(pg_id, ev_tick_running),
                        CsIntExpr::new_var(tick_response),
                    )?,
                )?;
                self.cs.add_transition(
                    pg_id,
                    loc_response,
                    step,
                    pt_failure,
                    CsFormula::eq(
                        CsIntExpr::new_const(pg_id, ev_tick_failure),
                        CsIntExpr::new_var(tick_response),
                    )?,
                )?;
            }
        }

        Ok(())
    }

    // fn build_bt_node(
    //     &mut self,
    //     pg_id: PgId,
    //     loc_tick: CsLocation,
    //     loc_success: CsLocation,
    //     loc_running: CsLocation,
    //     loc_failure: CsLocation,
    //     loc_halt: CsLocation,
    //     loc_ack: CsLocation,
    //     node: &BtNode,
    // ) -> anyhow::Result<(CsLocation, CsLocation)> {
    //     let tick_in = self.cs.new_location(pg_id)?;
    //     let tick_out = self.cs.new_location(pg_id)?;

    //     match node {
    //         BtNode::RSeq(branches) => {}
    //         BtNode::RFbk(branches) => {}
    //         BtNode::MSeq(branches) => {}
    //         BtNode::MFbk(branches) => {}
    //         BtNode::Invr(branch) => {
    //             // Recursively build branch BT and get its PoE location.
    //             let (tick_child_in, tick_child_out) =
    //                 self.build_bt_node(pg_id, tick_response, receive_response, branch)?;
    //             // Propagate the tick to the branch.
    //             let tick = self.cs.new_action(pg_id)?;
    //             self.cs.add_transition(
    //                 pg_id,
    //                 tick_in,
    //                 tick,
    //                 tick_child_in,
    //                 CsFormula::new_true(pg_id),
    //             )?;
    //             // Inverting the tick_response:
    //             // -1 ->  1
    //             //  0 ->  0
    //             //  1 -> -1
    //             let invert = self.cs.new_action(pg_id)?;
    //             self.cs.add_effect(
    //                 pg_id,
    //                 invert,
    //                 tick_response,
    //                 CsExpr::from_expr(CsIntExpr::opposite(CsIntExpr::new_var(tick_response))),
    //             )?;
    //             self.cs.add_transition(
    //                 pg_id,
    //                 tick_child_out,
    //                 invert,
    //                 tick_out,
    //                 CsFormula::new_true(pg_id),
    //             )?;
    //         }
    //         BtNode::LAct(id) | BtNode::LCnd(id) => {
    //             // Build external queue for skill, if it does not exist already.
    //             let external_queue = self.external_queues.get(id).cloned().unwrap_or_else(|| {
    //                 let external_queue = self.cs.new_channel(VarType::Integer, None);
    //                 self.external_queues.insert(id.to_owned(), external_queue);
    //                 external_queue
    //             });
    //             // Create tick event, if it does not exist already.
    //             let tick_call_event = self.get_event_idx(TICK_CALL);
    //             // Send a tickCall event to the skill.
    //             let tick_skill = self.cs.new_communication(
    //                 pg_id,
    //                 external_queue,
    //                 Message::Send(CsExpr::from_expr(CsIntExpr::new_const(
    //                     pg_id,
    //                     tick_call_event,
    //                 ))),
    //             )?;
    //             let wait_response_loc = self.cs.new_location(pg_id)?;
    //             self.cs.add_transition(
    //                 pg_id,
    //                 tick_in,
    //                 tick_skill,
    //                 wait_response_loc,
    //                 CsFormula::new_true(pg_id),
    //             )?;
    //             // Now leaf waits for response on its own channel
    //             self.cs.add_transition(
    //                 pg_id,
    //                 wait_response_loc,
    //                 receive_response,
    //                 tick_out,
    //                 CsFormula::new_true(pg_id),
    //             )?;
    //         }
    //     }

    //     Ok((tick_in, tick_out))
    // }

    // TODO: Optimize CS by removing unnecessary states
    fn build_fsm(&mut self, fsm: &Fsm) -> anyhow::Result<()> {
        trace!("build fsm {}", fsm.id);
        // Initialize fsm.
        let pg_id = self.get_moc_pgid(&fsm.id);
        let pg_idx = *self.moc_ids.get(&fsm.id).expect("should exist") as Integer;
        // Initial location of Program Graph.
        let initial_loc = self.cs.initial_location(pg_id)?;
        let initialize = self.cs.new_action(pg_id)?;
        // Initialize variables from datamodel
        for (location, (type_name, expr)) in fsm.datamodel.iter() {
            let scan_type = self
                .scan_types
                .get(type_name)
                .ok_or(anyhow!("unknown type"))?;
            let var = self.cs.new_var(pg_id, scan_type.to_owned())?;
            self.vars
                .insert((pg_id, location.to_owned()), (var, scan_type.to_owned()));
            // Initialize variable with `expr`, if any, by adding it as effect of `initialize` action.
            if let Some(expr) = expr {
                let expr = self.expression(pg_id, expr, &fsm.interner)?;
                self.cs.add_effect(pg_id, initialize, var, expr)?;
            }
        }
        // Transition initializing datamodel variables.
        // After initializing datamodel, transition to location representing point-of-entry of initial state of State Chart.
        let initial_state = self.cs.new_location(pg_id)?;
        self.cs.add_transition(
            pg_id,
            initial_loc,
            initialize,
            initial_state,
            CsFormula::new_true(pg_id),
        )?;
        // Map fsm's state ids to corresponding CS's locations.
        let mut states = HashMap::new();
        // Conventionally, the entry-point for a state is a location associated to the id of the state.
        // In particular, the id of the initial state of the fsm has to correspond to the initial location of the program graph.
        states.insert(fsm.initial.to_owned(), initial_state);
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
                onentry_loc = self.add_executable(
                    executable,
                    pg_id,
                    pg_idx,
                    int_queue,
                    onentry_loc,
                    &fsm.interner,
                )?;
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
                // Condition activating the transition.
                // It has to be parsed/built as a Boolean expression.
                let cond: Option<CsFormula> = if let Some(cond) = &transition.cond {
                    let cond = self.expression(pg_id, cond, &fsm.interner)?;
                    Some(
                        cond.try_into()
                            .expect("cond was built as a Boolean expression"),
                    )
                } else {
                    None
                };
                // Guard for transition.
                // Has to be defined depending on the type of transition, etc...
                let guard;
                // Proceed on whether the transition is eventless or activated by event.
                if let Some(event) = &transition.event {
                    // Create tick event, if it does not exist already.
                    let event_idx = self.get_event_idx(event);
                    // Check if the current event (internal or external) corresponds to the event activating the transition.
                    let event_match = CsFormula::eq(
                        CsIntExpr::new_var(current_event),
                        CsIntExpr::new_const(pg_id, event_idx),
                    )?;
                    guard = if let Some(cond) = cond {
                        CsFormula::and(event_match.clone(), cond).expect("formulae in same PG")
                    } else {
                        event_match
                    };
                    // Check this transition after the other eventful transitions.
                    check_trans_loc = eventful_trans;
                    // Move location of next eventful transitions to a new location.
                    eventful_trans = next_trans_loc;
                } else {
                    // // NULL (unnamed) event transition
                    // No event needs to happen in order to trigger this transition.
                    guard = cond.unwrap_or_else(|| CsFormula::new_true(pg_id));
                    // Check this transition after the other eventless transitions.
                    check_trans_loc = null_trans;
                    // Move location of next eventless transitions to a new location.
                    null_trans = next_trans_loc;
                }

                // If transition is active, execute the relevant executable content and then the transition to the target.
                let mut exec_trans_loc = self.cs.new_location(pg_id)?;
                self.cs.add_transition(
                    pg_id,
                    check_trans_loc,
                    exec_transition,
                    exec_trans_loc,
                    guard.to_owned(),
                )?;
                // Before executing executable content, we need to load the event's origin and parameters
                // This is done as an effect

                // First execute the executable content of the state's `on_exit` tag,
                // then that of the `transition` tag.
                for exec in state.on_exit.iter().chain(transition.effects.iter()) {
                    exec_trans_loc = self.add_executable(
                        exec,
                        pg_id,
                        pg_idx,
                        int_queue,
                        exec_trans_loc,
                        &fsm.interner,
                    )?;
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
        interner: &boa_interner::Interner,
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
                let target_ext_queue_origin = self.get_origin_channel(target_id);
                let send_origin = self.cs.new_communication(
                    pg_id,
                    target_ext_queue_origin,
                    crate::Message::Send(CsExpr::from_expr(CsIntExpr::new_const(pg_id, pg_idx))),
                )?;

                // Send event and event origin before moving on to next location.
                let event_loc = self.cs.new_location(pg_id)?;
                self.cs.add_transition(
                    pg_id,
                    loc,
                    send_event,
                    event_loc,
                    CsFormula::new_true(pg_id),
                )?;
                let mut next_loc = self.cs.new_location(pg_id)?;
                self.cs.add_transition(
                    pg_id,
                    event_loc,
                    send_origin,
                    next_loc,
                    CsFormula::new_true(pg_id),
                )?;

                // Pass parameters.
                for param in params {
                    // Updates next location.
                    next_loc =
                        self.send_param(pg_id, target_id, param, event_idx, next_loc, interner)?;
                }

                Ok(next_loc)
            }
            Executable::Assign { location, expr } => {
                // Add a transition that perform the assignment via the effect of the `assign` action.
                let expr = self.expression(pg_id, expr, interner)?;
                let (var, scan_type) = self
                    .vars
                    .get(&(pg_id, location.to_owned()))
                    .ok_or(anyhow!("undefined variable"))?;
                let assign = self.cs.new_action(pg_id)?;
                self.cs.add_effect(pg_id, assign, *var, expr)?;
                let next_loc = self.cs.new_location(pg_id)?;
                self.cs
                    .add_transition(pg_id, loc, assign, next_loc, CsFormula::new_true(pg_id))?;
                Ok(next_loc)
            }
        }
    }

    fn send_param(
        &mut self,
        pg_id: PgId,
        target_id: PgId,
        param: &Param,
        event_idx: i32,
        next_loc: CsLocation,
        interner: &boa_interner::Interner,
    ) -> Result<CsLocation, anyhow::Error> {
        // Get param type.
        let scan_type = self
            .scan_types
            .get(&param.omg_type)
            .cloned()
            .ok_or(anyhow!("Undefined type"))?;
        // Build expression from ECMAScript expression.
        let expr = self.expression(pg_id, &param.expr, interner)?;
        // Create channel for parameter passing.
        let param_chn = *self
            .parameters
            .entry((pg_id, target_id, event_idx, param.name.to_owned()))
            .or_insert(self.cs.new_channel(scan_type, None));

        let pass_param = self
            .cs
            .new_communication(pg_id, param_chn, Message::Send(expr))?;
        let param_loc = next_loc;
        let next_loc = self.cs.new_location(pg_id)?;
        self.cs.add_transition(
            pg_id,
            param_loc,
            pass_param,
            next_loc,
            CsFormula::new_true(pg_id),
        )?;
        Ok(next_loc)
    }

    fn expression(
        &mut self,
        pg_id: PgId,
        expr: &boa_ast::Expression,
        interner: &boa_interner::Interner,
        // scan_type: &VarType,
    ) -> anyhow::Result<CsExpr> {
        let expr = match expr {
            boa_ast::Expression::This => todo!(),
            boa_ast::Expression::Identifier(ident) => {
                let ident: &str = interner
                    .resolve(ident.sym())
                    .ok_or(anyhow!("unknown identifier"))?
                    .utf8()
                    .ok_or(anyhow!("not utf8"))?;
                match ident {
                    "_event" => todo!(),
                    var_ident => {
                        let (var, var_type) = self
                            .vars
                            .get(&(pg_id, var_ident.to_string()))
                            .ok_or(anyhow!("unknown variable"))?;
                        match var_type {
                            VarType::Unit => todo!(),
                            VarType::Boolean => CsExpr::from_formula(CsFormula::new_var(*var)),
                            VarType::Integer => CsExpr::from_expr(CsIntExpr::new_var(*var)),
                            VarType::Product(_) => todo!(),
                        }
                    }
                }
            }
            boa_ast::Expression::Literal(lit) => {
                use boa_ast::expression::literal::Literal;
                match lit {
                    Literal::String(_) => todo!(),
                    Literal::Num(_) => todo!(),
                    Literal::Int(i) => CsExpr::from_expr(CsIntExpr::new_const(pg_id, *i)),
                    Literal::BigInt(_) => todo!(),
                    Literal::Bool(b) => CsExpr::from_formula(CsFormula::new_const(pg_id, *b)),
                    Literal::Null => todo!(),
                    Literal::Undefined => todo!(),
                }
            }
            boa_ast::Expression::RegExpLiteral(_) => todo!(),
            boa_ast::Expression::ArrayLiteral(_) => todo!(),
            boa_ast::Expression::ObjectLiteral(_) => todo!(),
            boa_ast::Expression::Spread(_) => todo!(),
            boa_ast::Expression::Function(_) => todo!(),
            boa_ast::Expression::ArrowFunction(_) => todo!(),
            boa_ast::Expression::AsyncArrowFunction(_) => todo!(),
            boa_ast::Expression::Generator(_) => todo!(),
            boa_ast::Expression::AsyncFunction(_) => todo!(),
            boa_ast::Expression::AsyncGenerator(_) => todo!(),
            boa_ast::Expression::Class(_) => todo!(),
            boa_ast::Expression::TemplateLiteral(_) => todo!(),
            boa_ast::Expression::PropertyAccess(prop_acc) => {
                use boa_ast::expression::access::{PropertyAccess, PropertyAccessField};
                match prop_acc {
                    PropertyAccess::Simple(simp_prop_acc) => match simp_prop_acc.field() {
                        PropertyAccessField::Const(sym) => {
                            let ident: &str = interner
                                .resolve(*sym)
                                .ok_or(anyhow!("unknown identifier"))?
                                .utf8()
                                .ok_or(anyhow!("not utf8"))?;
                            match ident {
                                "origin" => {
                                    let queue_origin = self.get_origin_channel(pg_id);
                                    todo!()
                                }
                                var_ident => {
                                    // let (var, var_type) = self
                                    //     .vars
                                    //     .get(&(pg_id, var_ident.to_string()))
                                    //     .ok_or(anyhow!("unknown variable"))?;
                                    // let (var, var_type) = self
                                    // .parameters.get((???, pg_id, ))
                                    // match var_type {
                                    //     VarType::Unit => todo!(),
                                    //     VarType::Boolean => {
                                    //         CsExpr::from_formula(CsFormula::new_var(*var))
                                    //     }
                                    //     VarType::Integer => {
                                    //         CsExpr::from_expr(CsIntExpr::new_var(*var))
                                    //     }
                                    //     VarType::Product(_) => todo!(),
                                    // }
                                    todo!()
                                }
                            }
                        }
                        PropertyAccessField::Expr(_) => todo!(),
                    },
                    PropertyAccess::Private(_) => todo!(),
                    PropertyAccess::Super(_) => todo!(),
                }
            }
            boa_ast::Expression::New(_) => todo!(),
            boa_ast::Expression::Call(_) => todo!(),
            boa_ast::Expression::SuperCall(_) => todo!(),
            boa_ast::Expression::ImportCall(_) => todo!(),
            boa_ast::Expression::Optional(_) => todo!(),
            boa_ast::Expression::TaggedTemplate(_) => todo!(),
            boa_ast::Expression::NewTarget => todo!(),
            boa_ast::Expression::ImportMeta => todo!(),
            boa_ast::Expression::Assign(_) => todo!(),
            boa_ast::Expression::Unary(_) => todo!(),
            boa_ast::Expression::Update(_) => todo!(),
            boa_ast::Expression::Binary(bin) => {
                use boa_ast::expression::operator::binary::{ArithmeticOp, BinaryOp, RelationalOp};
                match bin.op() {
                    BinaryOp::Arithmetic(ar_bin) => {
                        let lhs =
                            CsIntExpr::try_from(self.expression(pg_id, bin.lhs(), interner)?)?;
                        let rhs =
                            CsIntExpr::try_from(self.expression(pg_id, bin.rhs(), interner)?)?;
                        match ar_bin {
                            ArithmeticOp::Add => CsExpr::from_expr(CsIntExpr::sum(lhs, rhs)?),
                            ArithmeticOp::Sub => todo!(),
                            ArithmeticOp::Div => todo!(),
                            ArithmeticOp::Mul => todo!(),
                            ArithmeticOp::Exp => todo!(),
                            ArithmeticOp::Mod => todo!(),
                        }
                    }
                    BinaryOp::Bitwise(_) => todo!(),
                    BinaryOp::Relational(rel_bin) => {
                        // WARN FIXME TODO: this assumes relations are between integers
                        let lhs =
                            CsIntExpr::try_from(self.expression(pg_id, bin.lhs(), interner)?)?;
                        let rhs =
                            CsIntExpr::try_from(self.expression(pg_id, bin.rhs(), interner)?)?;
                        let formula = match rel_bin {
                            RelationalOp::Equal => CsFormula::eq(lhs, rhs)?,
                            RelationalOp::NotEqual => todo!(),
                            RelationalOp::StrictEqual => todo!(),
                            RelationalOp::StrictNotEqual => todo!(),
                            RelationalOp::GreaterThan => CsFormula::greater(lhs, rhs)?,
                            RelationalOp::GreaterThanOrEqual => CsFormula::geq(lhs, rhs)?,
                            RelationalOp::LessThan => CsFormula::less(lhs, rhs)?,
                            RelationalOp::LessThanOrEqual => CsFormula::leq(lhs, rhs)?,
                            RelationalOp::In => todo!(),
                            RelationalOp::InstanceOf => todo!(),
                        };
                        CsExpr::from_formula(formula)
                    }
                    BinaryOp::Logical(_) => todo!(),
                    BinaryOp::Comma => todo!(),
                }
            }
            boa_ast::Expression::BinaryInPrivate(_) => todo!(),
            boa_ast::Expression::Conditional(_) => todo!(),
            boa_ast::Expression::Await(_) => todo!(),
            boa_ast::Expression::Yield(_) => todo!(),
            boa_ast::Expression::Parenthesized(_) => todo!(),
            _ => todo!(),
        };
        Ok(expr)
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
