use std::{collections::HashMap, str::FromStr};

use crate::{parser::vocabulary::*, CsAction, Val};
use anyhow::{anyhow, Ok, Result};
use log::{info, trace};

use crate::{
    parser::*, Channel, ChannelSystem, ChannelSystemBuilder, CsExpression, CsLocation, CsVar,
    Integer, Message, PgId, Type,
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
    scan_types: HashMap<String, Type>,
    enums: HashMap<(String, String), Integer>,
    // Each State Chart has an associated Program Graph,
    // and an arbitrary, progressive index
    moc_ids: HashMap<String, usize>,
    moc_pgid: Vec<PgId>,
    // skill_ids: HashMap<String, PgId>,
    // component_ids: HashMap<String, PgId>,
    // Each State Chart has an associated external event queue.
    channels: HashMap<String, Channel>,
    // Each event is associated to a unique global index.
    events: HashMap<String, Integer>,
    // For each State Chart, each variable is associated to an index.
    vars: HashMap<(PgId, String), (CsVar, Type)>,
    // Events carrying parameters have dedicated channels for them,
    // one for each:
    // - senderStateChart
    // - receiverStateChart
    // - sentEvent (index)
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
            channels: HashMap::new(),
            events: HashMap::new(),
            parameters: HashMap::new(),
            vars: HashMap::new(),
        };

        // FIXME: Is there a better way? Const object?
        model
            .scan_types
            .insert(String::from_str("int32").unwrap(), Type::Integer);
        model
            .scan_types
            .insert(String::from_str("URI").unwrap(), Type::Integer);
        model
            .scan_types
            .insert(String::from_str("Boolean").unwrap(), Type::Boolean);

        model.build_types(&parser.types)?;

        model.build_channels(&parser)?;

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
                OmgType::Boolean => Type::Boolean,
                OmgType::Int32 => Type::Integer,
                OmgType::Structure() => todo!(),
                OmgType::Enumeration(labels) => {
                    for (idx, label) in labels.iter().enumerate() {
                        self.enums
                            .insert((name.to_owned(), label.to_owned()), idx as Integer);
                    }
                    Type::Integer
                }
            };
            self.scan_types.insert(name.to_owned(), scan_type);
        }
        Ok(())
    }

    fn event_idx(&mut self, id: &str) -> Integer {
        self.events.get(id).cloned().unwrap_or_else(|| {
            let idx = self.events.len() as Integer;
            self.events.insert(id.to_owned(), idx);
            idx
        })
    }

    fn moc_pgid(&mut self, id: &str) -> PgId {
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

    fn channel(&mut self, id: &str, ch_type: Type, capacity: Option<usize>) -> Channel {
        self.channels.get(id).cloned().unwrap_or_else(|| {
            let channel = self.cs.new_channel(ch_type, capacity);
            self.channels.insert(id.to_owned(), channel);
            channel
        })
    }

    fn external_queue(&mut self, pg_id: PgId) -> Channel {
        let ext_queue_name = format!("BLDR:{pg_id:?}:external_queue");
        self.channel(
            &ext_queue_name,
            Type::Product(vec![Type::Integer, Type::Integer]),
            None,
        )
    }

    fn var(&self, pg_id: PgId, id: &str) -> anyhow::Result<(CsVar, Type)> {
        self.vars
            .get(&(pg_id, id.to_string()))
            .cloned()
            .ok_or(anyhow!("non-existing variable"))
    }

    fn new_var(&mut self, pg_id: PgId, id: &str, var_type: Type) -> anyhow::Result<CsVar> {
        if self.vars.contains_key(&(pg_id, id.to_owned())) {
            Err(anyhow!("variable named {id} already exists"))
        } else {
            let var = self.cs.new_var(pg_id, var_type.to_owned())?;
            self.vars.insert((pg_id, id.to_string()), (var, var_type));
            Ok(var)
        }
    }

    // fn param_channel(
    //     &mut self,
    //     sender: PgId,
    //     receiver: PgId,
    //     event_name: String,
    //     ident: String,
    //     var_type: Type,
    // ) -> Channel {
    //     let param = format!("BLDR_CH:{sender:?}:{receiver:?}:{event_name}:{ident}");
    //     self.channel(&param, var_type, None)
    // }

    fn param_var(
        &mut self,
        pg_id: PgId,
        event_name: String,
        ident: String,
        var_type: Type,
    ) -> anyhow::Result<CsVar> {
        let var_name = format!("BLDR_VAR:{pg_id:?}:{event_name}:{ident}");
        self.var(pg_id, &var_name)
            .map(|(v, _)| v)
            .or_else(|_| self.new_var(pg_id, &var_name, var_type))
    }

    fn build_channels(&mut self, parser: &Parser) -> anyhow::Result<()> {
        for (id, declaration) in parser.process_list.iter() {
            let pg_id = self.moc_pgid(id);
            match &declaration.moc {
                MoC::Fsm(fsm) => self.build_channels_fsm(pg_id, fsm)?,
                MoC::Bt(bt) => self.build_channels_bt(pg_id, bt)?,
            }
        }
        Ok(())
    }

    fn build_channels_fsm(&mut self, pg_id: PgId, fmt: &Fsm) -> anyhow::Result<()> {
        for (_, state) in fmt.states.iter() {
            for exec in state.on_entry.iter() {
                self.build_channels_exec(pg_id, exec)?;
            }
            for transition in state.transitions.iter() {
                for exec in transition.effects.iter() {
                    self.build_channels_exec(pg_id, exec)?;
                }
            }
            for exec in state.on_exit.iter() {
                self.build_channels_exec(pg_id, exec)?;
            }
        }
        Ok(())
    }

    fn build_channels_exec(&mut self, pg_id: PgId, executable: &Executable) -> anyhow::Result<()> {
        match executable {
            Executable::Assign {
                location: _,
                expr: _,
            } => Ok(()),
            Executable::Raise { event: _ } => Ok(()),
            Executable::Send {
                event,
                target,
                params,
            } => {
                let event_id = self.event_idx(event);
                let target_id = self.moc_pgid(target);
                for param in params {
                    let var_type = self
                        .scan_types
                        .get(&param.omg_type)
                        .ok_or(anyhow!("type not found"))?
                        .clone();
                    self.parameters
                        .entry((pg_id, target_id, event_id, param.name.to_owned()))
                        .or_insert(self.cs.new_channel(var_type, None));
                }
                Ok(())
            }
        }
    }

    fn build_channels_bt(&mut self, pg_id: PgId, bt: &Bt) -> anyhow::Result<()> {
        Ok(())
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
    //     let tick_event = self.cs.new_var(pg_id, Type::Integer)?;
    //     let receive_tick =
    //         self.cs
    //             .new_communication(pg_id, tick_receive_chn, Message::Receive(tick_event))?;
    //     // TODO: implement tick_response with enum type.
    //     // tick_response, manually implemented:
    //     // -1 = Failure
    //     //  0 = Running (default value)
    //     //  1 = Success
    //     let tick_response = self.cs.new_var(pg_id, Type::Integer)?;
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
    //         None,
    //     )?;
    //     Ok(())
    // }

    fn build_bt(&mut self, bt: &Bt) -> anyhow::Result<()> {
        trace!("build bt {}", bt.id);
        // Initialize bt.
        let pg_id = self.moc_pgid(&bt.id);
        let loc_tick = self.cs.initial_location(pg_id)?;
        let loc_success = self.cs.new_location(pg_id)?;
        let loc_running = self.cs.new_location(pg_id)?;
        let loc_failure = self.cs.new_location(pg_id)?;
        let loc_halt = self.cs.new_location(pg_id)?;
        let loc_ack = self.cs.new_location(pg_id)?;
        let step = self.cs.new_action(pg_id)?;
        self.cs
            .add_transition(pg_id, loc_running, step, loc_tick, None)?;
        let tick_response_chn = self.external_queue(pg_id);
        let tick_response = self.cs.new_var(pg_id, Type::Integer)?;
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
                    self.cs
                        .add_transition(pg_id, prev_success, step, loc_tick, None)?;
                    self.cs
                        .add_transition(pg_id, loc_running, step, pt_running, None)?;
                    self.cs
                        .add_transition(pg_id, prev_failure, step, loc_halt_failure, None)?;
                    self.cs.add_transition(
                        pg_id,
                        prev_ack_failure,
                        step,
                        loc_halt_failure,
                        None,
                    )?;
                    self.cs
                        .add_transition(pg_id, prev_ack, step, loc_halt, None)?;
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
                self.cs
                    .add_transition(pg_id, prev_success, step, pt_success, None)?;
                self.cs
                    .add_transition(pg_id, prev_ack, step, pt_ack, None)?;
                self.cs
                    .add_transition(pg_id, prev_ack_failure, step, pt_ack_failure, None)?;
                // self.cs.add_transition(
                //     pg_id,
                //     prev_ack_success,
                //     step,
                //     pt_ack_success,
                //     None,
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
                //     None,
                // )?;
                // self.cs.add_transition(
                //     pg_id,
                //     loc_running,
                //     step,
                //     pt_running,
                //     None,
                // )?;
                // self.cs.add_transition(
                //     pg_id,
                //     loc_success,
                //     step,
                //     pt_failure,
                //     None,
                // )?;
                // self.cs.add_transition(
                //     pg_id,
                //     pt_halt,
                //     step,
                //     loc_halt,
                //     None,
                // )?;
                // self.cs
                //     .add_transition(pg_id, loc_ack, step, pt_ack, None)?;
            }
            BtNode::LAct(id) | BtNode::LCnd(id) => {
                // let ev_tick = self.get_event_idx(&"TICK");
                let ev_tick_success = self.event_idx(&"SUCCESS");
                let ev_tick_running = self.event_idx(&"RUNNING");
                let ev_tick_failure = self.event_idx(&"FAILURE");
                // let ev_halt = self.get_event_idx(&"HALT");
                // let ev_halt_ack = self.get_event_idx(&"ACK");
                let loc_tick = self.cs.new_location(pg_id)?;
                let loc_response = self.cs.new_location(pg_id)?;
                // Build external queue for skill, if it does not exist already.
                let skill_id = self.moc_pgid(id);
                let external_queue = self.external_queue(skill_id);
                // Create tick event, if it does not exist already.
                let tick_call_event = self.event_idx(TICK_CALL);
                // Send a tickCall event to the skill.
                let tick_skill = self.cs.new_communication(
                    pg_id,
                    external_queue,
                    Message::Send(CsExpression::Const(Val::Integer(tick_call_event))),
                )?;
                self.cs
                    .add_transition(pg_id, pt_tick, tick_skill, loc_tick, None)?;
                // Now leaf waits for response on its own channel
                self.cs
                    .add_transition(pg_id, loc_tick, receive_response, loc_response, None)?;
                self.cs.add_transition(
                    pg_id,
                    loc_response,
                    step,
                    pt_success,
                    Some(CsExpression::Equal(Box::new((
                        CsExpression::Const(Val::Integer(ev_tick_success)),
                        CsExpression::Var(tick_response),
                    )))),
                )?;
                self.cs.add_transition(
                    pg_id,
                    loc_response,
                    step,
                    pt_running,
                    Some(CsExpression::Equal(Box::new((
                        CsExpression::Const(Val::Integer(ev_tick_running)),
                        CsExpression::Var(tick_response),
                    )))),
                )?;
                self.cs.add_transition(
                    pg_id,
                    loc_response,
                    step,
                    pt_failure,
                    Some(CsExpression::Equal(Box::new((
                        CsExpression::Const(Val::Integer(ev_tick_failure)),
                        CsExpression::Var(tick_response),
                    )))),
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
    //                 None,
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
    //                 None,
    //             )?;
    //         }
    //         BtNode::LAct(id) | BtNode::LCnd(id) => {
    //             // Build external queue for skill, if it does not exist already.
    //             let external_queue = self.external_queues.get(id).cloned().unwrap_or_else(|| {
    //                 let external_queue = self.cs.new_channel(Type::Integer, None);
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
    //                 None,
    //             )?;
    //             // Now leaf waits for response on its own channel
    //             self.cs.add_transition(
    //                 pg_id,
    //                 wait_response_loc,
    //                 receive_response,
    //                 tick_out,
    //                 None,
    //             )?;
    //         }
    //     }

    //     Ok((tick_in, tick_out))
    // }

    // TODO: Optimize CS by removing unnecessary states
    fn build_fsm(&mut self, fsm: &Fsm) -> anyhow::Result<()> {
        trace!("build fsm {}", fsm.id);
        // Initialize fsm.
        let pg_id = self.moc_pgid(&fsm.id);
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
                let expr = self.expression(pg_id, expr, &fsm.interner, None, None)?;
                self.cs.add_effect(pg_id, initialize, var, expr)?;
            }
        }
        // Transition initializing datamodel variables.
        // After initializing datamodel, transition to location representing point-of-entry of initial state of State Chart.
        let initial_state = self.cs.new_location(pg_id)?;
        self.cs
            .add_transition(pg_id, initial_loc, initialize, initial_state, None)?;
        // Map fsm's state ids to corresponding CS's locations.
        let mut states = HashMap::new();
        // Conventionally, the entry-point for a state is a location associated to the id of the state.
        // In particular, the id of the initial state of the fsm has to correspond to the initial location of the program graph.
        states.insert(fsm.initial.to_owned(), initial_state);
        // Var representing the current event and origin pair
        let current_event_and_origin = self
            .cs
            .new_var(pg_id, Type::Product(vec![Type::Integer, Type::Integer]))?;
        // Var representing the current event
        let current_event = self.cs.new_var(pg_id, Type::Integer)?;
        // Implement internal queue
        let int_queue = self.cs.new_channel(Type::Integer, None);
        let dequeue_int =
            self.cs
                .new_communication(pg_id, int_queue, Message::Receive(current_event))?;
        // Variable that will store origin of last processed event.
        let origin_var = self.cs.new_var(pg_id, Type::Integer)?;
        let set_int_origin = self.cs.new_action(pg_id)?;
        self.cs.add_effect(
            pg_id,
            set_int_origin,
            origin_var,
            CsExpression::Const(Val::Integer(pg_idx)),
        )?;
        // Implement external queue
        let ext_queue = self.external_queue(pg_id);
        let dequeue_ext = self.cs.new_communication(
            pg_id,
            ext_queue,
            Message::Receive(current_event_and_origin),
        )?;
        // Process external event to assign event and origin values to respective vars
        let process_ext_event = self.cs.new_action(pg_id)?;
        self.cs.add_effect(
            pg_id,
            process_ext_event,
            current_event,
            CsExpression::Component(0, Box::new(CsExpression::Var(current_event_and_origin))),
        )?;
        self.cs.add_effect(
            pg_id,
            process_ext_event,
            origin_var,
            CsExpression::Component(1, Box::new(CsExpression::Var(current_event_and_origin))),
        )?;
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
                    None,
                    None,
                    &fsm.interner,
                )?;
            }

            // Location where eventless/NULL transitions activate
            let mut null_trans = onentry_loc;
            // Location where internal events are dequeued
            let int_queue_loc = self.cs.new_location(pg_id)?;
            // Location where the origin of internal events is set as own.
            let int_origin_loc = self.cs.new_location(pg_id)?;
            // Location where external events are dequeued
            let ext_queue_loc = self.cs.new_location(pg_id)?;
            // Location where the origin of external events is dequeued
            let ext_queue_origin_loc = self.cs.new_location(pg_id)?;
            // Location where eventful transitions activate
            let mut eventful_trans = self.cs.new_location(pg_id)?;
            // Transition dequeueing a new internal event and searching for first active eventful transition
            self.cs
                .add_transition(pg_id, int_queue_loc, dequeue_int, int_origin_loc, None)?;
            // Transition dequeueing a new internal event and searching for first active eventful transition
            self.cs
                .add_transition(pg_id, int_origin_loc, set_int_origin, eventful_trans, None)?;
            // Action denoting checking if internal queue is empty;
            // if so, move to external queue.
            // Notice that one and only one of `int_dequeue` and `empty_int_queue` can be executed at a given time.
            let empty_int_queue =
                self.cs
                    .new_communication(pg_id, int_queue, crate::Message::ProbeEmptyQueue)?;
            self.cs
                .add_transition(pg_id, int_queue_loc, empty_int_queue, ext_queue_loc, None)?;
            // Dequeue a new external event and search for first active named transition.
            self.cs.add_transition(
                pg_id,
                ext_queue_loc,
                dequeue_ext,
                ext_queue_origin_loc,
                None,
            )?;
            // Dequeue the origin of the corresponding external event.
            self.cs.add_transition(
                pg_id,
                ext_queue_origin_loc,
                process_ext_event,
                eventful_trans,
                None,
            )?;
            // Retreive external event's parameters
            // todo!();

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
                let cond: Option<CsExpression> = if let Some(cond) = &transition.cond {
                    let cond = self.expression(pg_id, cond, &fsm.interner, None, None)?;
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
                    let event_idx = self.event_idx(event);
                    // Check if the current event (internal or external) corresponds to the event activating the transition.
                    let event_match = CsExpression::Equal(Box::new((
                        CsExpression::Var(current_event),
                        CsExpression::Const(Val::Integer(event_idx)),
                    )));
                    // TODO FIXME ugly code!
                    guard = Some(
                        cond.map(|cond| CsExpression::And(vec![event_match.clone(), cond]))
                            .unwrap_or(event_match),
                    );
                    // Check this transition after the other eventful transitions.
                    check_trans_loc = eventful_trans;
                    // Move location of next eventful transitions to a new location.
                    eventful_trans = next_trans_loc;
                } else {
                    // // NULL (unnamed) event transition
                    // No event needs to happen in order to trigger this transition.
                    guard = cond;
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
                let param: Option<(String, CsVar, Type)> =
                    if let Some((ref ident, ref omg_type)) = transition.param {
                        let var_type = self
                            .scan_types
                            .get(omg_type)
                            .ok_or(anyhow!("unknown type"))?
                            .to_owned();
                        let event_name = transition
                            .event
                            .clone()
                            .ok_or(anyhow!("unnamed events cannot have parameters"))?;
                        let param_var = self.param_var(
                            pg_id,
                            event_name.to_owned(),
                            ident.to_owned(),
                            var_type.to_owned(),
                        )?;
                        Some((ident.to_string(), param_var, var_type.to_owned()))
                    } else {
                        None
                    };
                // First execute the executable content of the state's `on_exit` tag,
                // then that of the `transition` tag.
                for exec in state.on_exit.iter().chain(transition.effects.iter()) {
                    exec_trans_loc = self.add_executable(
                        exec,
                        pg_id,
                        pg_idx,
                        int_queue,
                        exec_trans_loc,
                        Some(origin_var),
                        param.to_owned(),
                        &fsm.interner,
                    )?;
                }
                // Transitioning to the target state/location.
                // At this point, the transition cannot be stopped so there can be no guard.
                self.cs
                    .add_transition(pg_id, exec_trans_loc, exec_transition, target_loc, None)?;
                // If the current transition is not active, move on to check the next one.
                let not_guard = guard
                    .map(|guard| CsExpression::Not(Box::new(guard)))
                    .unwrap_or(CsExpression::Const(Val::Boolean(false)));
                self.cs.add_transition(
                    pg_id,
                    check_trans_loc,
                    next_transition,
                    next_trans_loc,
                    Some(not_guard),
                )?;
            }

            // Connect NULL events with named events
            // by transitioning from last "NUll" location to dequeuing event location.
            self.cs
                .add_transition(pg_id, null_trans, next_transition, int_queue_loc, None)?;
            // Return to dequeue a new (internal or external) event.
            self.cs
                .add_transition(pg_id, eventful_trans, next_transition, int_queue_loc, None)?;
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
        origin: Option<CsVar>,
        param: Option<(String, CsVar, Type)>,
        interner: &boa_interner::Interner,
    ) -> Result<CsLocation, anyhow::Error> {
        match executable {
            Executable::Raise { event } => {
                // Create event, if it does not exist already.
                let event_idx = self.event_idx(event);
                let raise = self.cs.new_communication(
                    pg_id,
                    int_queue,
                    crate::Message::Send(CsExpression::Const(Val::Integer(event_idx))),
                )?;
                let next_loc = self.cs.new_location(pg_id)?;
                // queue the internal event
                self.cs.add_transition(pg_id, loc, raise, next_loc, None)?;
                Ok(next_loc)
            }
            Executable::Send {
                event,
                target,
                params,
            } => {
                let target_id = self.moc_pgid(target);
                // Create event, if it does not exist already.
                let event_idx = self.event_idx(event);
                let target_ext_queue = self.external_queue(target_id);
                let send_event = self.cs.new_communication(
                    pg_id,
                    target_ext_queue,
                    crate::Message::Send(CsExpression::Const(Val::Tuple(vec![
                        Val::Integer(event_idx),
                        Val::Integer(pg_idx),
                    ]))),
                )?;

                // Send event and event origin before moving on to next location.
                let mut next_loc = self.cs.new_location(pg_id)?;
                self.cs
                    .add_transition(pg_id, loc, send_event, next_loc, None)?;

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
                let expr = self.expression(pg_id, expr, interner, origin, param)?;
                let (var, scan_type) = self
                    .vars
                    .get(&(pg_id, location.to_owned()))
                    .ok_or(anyhow!("undefined variable"))?;
                let assign = self.cs.new_action(pg_id)?;
                self.cs.add_effect(pg_id, assign, *var, expr)?;
                let next_loc = self.cs.new_location(pg_id)?;
                self.cs.add_transition(pg_id, loc, assign, next_loc, None)?;
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
            .ok_or(anyhow!("undefined type"))?;
        // Build expression from ECMAScript expression.
        let expr = self.expression(pg_id, &param.expr, interner, None, None)?;
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
        self.cs
            .add_transition(pg_id, param_loc, pass_param, next_loc, None)?;
        Ok(next_loc)
    }

    fn expression(
        &mut self,
        pg_id: PgId,
        expr: &boa_ast::Expression,
        interner: &boa_interner::Interner,
        origin: Option<CsVar>,
        param: Option<(String, CsVar, Type)>,
    ) -> anyhow::Result<CsExpression> {
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
                    var_ident => self
                        .vars
                        .get(&(pg_id, var_ident.to_string()))
                        .ok_or(anyhow!("unknown variable"))
                        .map(|(var, _)| CsExpression::Var(*var))?,
                }
            }
            boa_ast::Expression::Literal(lit) => {
                use boa_ast::expression::literal::Literal;
                CsExpression::Const(match lit {
                    Literal::String(_) => todo!(),
                    Literal::Num(_) => todo!(),
                    Literal::Int(i) => Val::Integer(*i),
                    Literal::BigInt(_) => todo!(),
                    Literal::Bool(b) => Val::Boolean(*b),
                    Literal::Null => todo!(),
                    Literal::Undefined => todo!(),
                })
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
                        // FIXME WARN this makes overly simplified assumptions on field access and will not work with complex types
                        PropertyAccessField::Const(sym) => {
                            let ident: &str = interner
                                .resolve(*sym)
                                .ok_or(anyhow!("unknown identifier"))?
                                .utf8()
                                .ok_or(anyhow!("not utf8"))?;
                            match ident {
                                "origin" => {
                                    let origin = origin.ok_or(anyhow!("origin not available"))?;
                                    CsExpression::Var(origin)
                                }
                                var_ident => {
                                    if let Some((param_ident, param_var, var_type)) = param {
                                        if param_ident == var_ident {
                                            match var_type {
                                                Type::Unit => todo!(),
                                                Type::Boolean => CsExpression::Var(param_var),
                                                Type::Integer => CsExpression::Var(param_var),
                                                Type::Product(_) => todo!(),
                                            }
                                        } else {
                                            return Err(anyhow!("parameter not found"));
                                        }
                                    } else {
                                        return Err(anyhow!("no parameter found"));
                                    }
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
                            self.expression(pg_id, bin.lhs(), interner, origin, param.to_owned())?;
                        let rhs = self.expression(pg_id, bin.rhs(), interner, origin, param)?;
                        match ar_bin {
                            ArithmeticOp::Add => CsExpression::Sum(vec![lhs, rhs]),
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
                            self.expression(pg_id, bin.lhs(), interner, origin, param.to_owned())?;
                        let rhs = self.expression(pg_id, bin.rhs(), interner, origin, param)?;
                        match rel_bin {
                            RelationalOp::Equal => CsExpression::Equal(Box::new((lhs, rhs))),
                            RelationalOp::NotEqual => todo!(),
                            RelationalOp::StrictEqual => todo!(),
                            RelationalOp::StrictNotEqual => todo!(),
                            RelationalOp::GreaterThan => {
                                CsExpression::Greater(Box::new((lhs, rhs)))
                            }
                            RelationalOp::GreaterThanOrEqual => {
                                CsExpression::GreaterEq(Box::new((lhs, rhs)))
                            }
                            RelationalOp::LessThan => CsExpression::Less(Box::new((lhs, rhs))),
                            RelationalOp::LessThanOrEqual => {
                                CsExpression::LessEq(Box::new((lhs, rhs)))
                            }
                            RelationalOp::In => todo!(),
                            RelationalOp::InstanceOf => todo!(),
                        }
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
