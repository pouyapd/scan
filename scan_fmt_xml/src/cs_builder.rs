use crate::parser::*;
use anyhow::anyhow;
use log::{info, trace};
use scan_core::{channel_system::*, *};
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct CsModel {
    pub cs: ChannelSystem,
    pub fsm_names: HashMap<PgId, String>,
}

#[derive(Debug, Clone)]
struct FsmBuilder {
    pg_id: PgId,
    ext_queue: Channel,
    index: usize,
}

#[derive(Debug, Clone)]
struct EventBuilder {
    params: HashMap<String, Type>,
    senders: HashSet<PgId>,
    receivers: HashSet<PgId>,
    index: usize,
}

#[derive(Debug)]
pub struct Sc2CsVisitor {
    cs: ChannelSystemBuilder,
    // Represent OMG types
    scan_types: HashMap<String, Type>,
    // WARN FIXME TODO: simplistic implementation of enums
    enums: HashMap<String, Integer>,
    // Each State Chart has an associated Program Graph,
    // and an arbitrary, progressive index
    fsm_builders: HashMap<String, FsmBuilder>,
    // Each event is associated to a unique global index and parameter(s).
    // WARN FIXME TODO: name clashes
    events: Vec<EventBuilder>,
    event_indexes: HashMap<String, usize>,
    // Events carrying parameters have dedicated channels for them,
    // one for each:
    // - senderStateChart
    // - receiverStateChart
    // - sentEvent (index)
    // - paramName
    // that is needed
    parameters: HashMap<(PgId, PgId, usize, String), Channel>,
}

impl Sc2CsVisitor {
    pub fn visit(parser: Parser) -> anyhow::Result<CsModel> {
        // Add base types
        // FIXME: Is there a better way? Const object?
        let base_types: [(String, Type); 3] = [
            (String::from("Boolean"), Type::Boolean),
            (String::from("int32"), Type::Integer),
            (String::from("URI"), Type::Integer),
        ];

        let mut model = Sc2CsVisitor {
            cs: ChannelSystemBuilder::new(),
            scan_types: HashMap::from_iter(base_types.into_iter()),
            enums: HashMap::new(),
            fsm_builders: HashMap::new(),
            events: Vec::new(),
            event_indexes: HashMap::new(),
            parameters: HashMap::new(),
        };

        model.build_types(&parser.types)?;

        model.prebuild_processes(&parser)?;

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
                        self.enums.insert(label.to_owned(), idx as Integer);
                    }
                    Type::Integer
                }
            };
            self.scan_types.insert(name.to_owned(), scan_type);
        }
        Ok(())
    }

    fn event_index(&mut self, id: &str) -> usize {
        self.event_indexes.get(id).cloned().unwrap_or_else(|| {
            let index = self.events.len();
            self.events.push(EventBuilder {
                params: HashMap::new(),
                index,
                senders: HashSet::new(),
                receivers: HashSet::new(),
            });
            self.event_indexes.insert(id.to_owned(), index);
            index
        })
    }

    fn fsm_builder(&mut self, id: &str) -> &FsmBuilder {
        if !self.fsm_builders.contains_key(id) {
            let index = self.fsm_builders.len();
            let pg_id = self.cs.new_program_graph();
            let ext_queue = self
                .cs
                .new_channel(Type::Product(vec![Type::Integer, Type::Integer]), None);
            let fsm = FsmBuilder {
                pg_id,
                ext_queue,
                index,
            };
            self.fsm_builders.insert(id.to_string(), fsm);
        }
        self.fsm_builders.get(id).expect("just inserted")
    }

    // fn get_or_add_param(
    //     &mut self,
    //     sender: PgId,
    //     receiver: PgId,
    //     event: usize,
    //     param: String,
    //     par_type: Type,
    // ) -> anyhow::Result<Channel> {
    //     todo!()
    // }

    // fn param_var(
    //     &mut self,
    //     pg_id: PgId,
    //     event_name: String,
    //     ident: String,
    //     var_type: Type,
    // ) -> anyhow::Result<CsVar> {
    //     let var_name = format!("BLDR_VAR:{pg_id:?}:{event_name}:{ident}");
    //     self.var(pg_id, &var_name)
    //         .map(|(v, _)| v)
    //         .or_else(|_| self.new_var(pg_id, &var_name, var_type))
    // }

    fn prebuild_processes(&mut self, parser: &Parser) -> anyhow::Result<()> {
        for (id, declaration) in parser.process_list.iter() {
            let pg_id = self.fsm_builder(id).pg_id;
            match &declaration.moc {
                MoC::Fsm(fsm) => self.prebuild_fsms(pg_id, fsm)?,
                MoC::Bt(bt) => self.build_channels_bt(pg_id, bt)?,
            }
        }
        Ok(())
    }

    fn prebuild_fsms(&mut self, pg_id: PgId, fmt: &Fsm) -> anyhow::Result<()> {
        for (_, state) in fmt.states.iter() {
            for exec in state.on_entry.iter() {
                self.prebuild_params(pg_id, exec)?;
            }
            for transition in state.transitions.iter() {
                if let Some(ref event) = transition.event {
                    // Event may or may not have been processed before
                    let event_index = self.event_index(event);
                    let builder = self.events.get_mut(event_index).expect("index must exist");
                    builder.receivers.insert(pg_id);
                }
                for exec in transition.effects.iter() {
                    self.prebuild_params(pg_id, exec)?;
                }
            }
            for exec in state.on_exit.iter() {
                self.prebuild_params(pg_id, exec)?;
            }
        }
        Ok(())
    }

    fn prebuild_params(&mut self, pg_id: PgId, executable: &Executable) -> anyhow::Result<()> {
        match executable {
            Executable::Assign {
                location: _,
                expr: _,
            } => Ok(()),
            Executable::Raise { event: _ } => Ok(()),
            Executable::Send {
                event,
                target: _,
                params,
            } => {
                let event_index = self.event_index(event);
                let builder = self.events.get_mut(event_index).expect("index must exist");
                builder.senders.insert(pg_id);
                for param in params {
                    let var_type = self
                        .scan_types
                        .get(&param.omg_type)
                        .ok_or(anyhow!("type not found"))?
                        .clone();
                    let prev_type = self.events[event_index]
                        .params
                        .insert(param.name.to_owned(), var_type.to_owned());
                    // Type parameters should not change type
                    if let Some(prev_type) = prev_type {
                        if prev_type != var_type {
                            return Err(anyhow!("type parameter mismatch"));
                        }
                    }
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
        let pg_builder = self.fsm_builder(&bt.id);
        let pg_id = pg_builder.pg_id;
        let loc_tick = self.cs.initial_location(pg_id)?;
        let loc_success = self.cs.new_location(pg_id)?;
        let loc_running = self.cs.new_location(pg_id)?;
        let loc_failure = self.cs.new_location(pg_id)?;
        let loc_halt = self.cs.new_location(pg_id)?;
        let loc_ack = self.cs.new_location(pg_id)?;
        let step = self.cs.new_action(pg_id)?;
        self.cs
            .add_transition(pg_id, loc_running, step, loc_tick, None)?;
        // let tick_response_chn = self.external_queue(pg_id);
        // let tick_response = self.cs.new_var(pg_id, Type::Integer)?;
        // let receive_response =
        //     self.cs
        //         .new_communication(pg_id, tick_response_chn, Message::Receive(tick_response))?;
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
                // let ev_tick_success = self.event_index(&"SUCCESS");
                // let ev_tick_running = self.event_index(&"RUNNING");
                // let ev_tick_failure = self.event_index(&"FAILURE");
                // let ev_halt = self.get_event_idx(&"HALT");
                // let ev_halt_ack = self.get_event_idx(&"ACK");
                // let loc_tick = self.cs.new_location(pg_id)?;
                // let loc_response = self.cs.new_location(pg_id)?;
                // // Build external queue for skill, if it does not exist already.
                // let skill_id = self.fsm_builder(id);
                // // let external_queue = self.external_queue(skill_id);
                // // Create tick event, if it does not exist already.
                // let tick_call_event = self.event_index(TICK_CALL);
                // // Send a tickCall event to the skill.
                // // let tick_skill = self.cs.new_communication(
                // //     pg_id,
                // //     external_queue,
                // //     Message::Send(CsExpression::Const(Val::Integer(tick_call_event))),
                // // )?;
                // // self.cs
                // //     .add_transition(pg_id, pt_tick, tick_skill, loc_tick, None)?;
                // // Now leaf waits for response on its own channel
                // self.cs
                //     .add_transition(pg_id, loc_tick, receive_response, loc_response, None)?;
                // self.cs.add_transition(
                //     pg_id,
                //     loc_response,
                //     step,
                //     pt_success,
                //     Some(CsExpression::Equal(Box::new((
                //         CsExpression::Const(Val::Integer(ev_tick_success)),
                //         CsExpression::Var(tick_response),
                //     )))),
                // )?;
                // self.cs.add_transition(
                //     pg_id,
                //     loc_response,
                //     step,
                //     pt_running,
                //     Some(CsExpression::Equal(Box::new((
                //         CsExpression::Const(Val::Integer(ev_tick_running)),
                //         CsExpression::Var(tick_response),
                //     )))),
                // )?;
                // self.cs.add_transition(
                //     pg_id,
                //     loc_response,
                //     step,
                //     pt_failure,
                //     Some(CsExpression::Equal(Box::new((
                //         CsExpression::Const(Val::Integer(ev_tick_failure)),
                //         CsExpression::Var(tick_response),
                //     )))),
                // )?;
            }
        }

        Ok(())
    }

    // TODO: Optimize CS by removing unnecessary states:
    // - initialize state if empty datamodel
    fn build_fsm(&mut self, fsm: &Fsm) -> anyhow::Result<()> {
        trace!("build fsm {}", fsm.id);
        // Initialize fsm.
        let pg_builder = self
            .fsm_builders
            .get(&fsm.id)
            .expect("builder must already exist");
        let pg_id = pg_builder.pg_id;
        let pg_index = pg_builder.index as Integer;
        let ext_queue = pg_builder.ext_queue;
        // Initial location of Program Graph.
        let initial_loc = self
            .cs
            .initial_location(pg_id)
            .expect("program graph must exist");
        let initialize = self.cs.new_action(pg_id).expect("program graph must exist");
        // Initialize variables from datamodel
        let mut vars = HashMap::new();
        for (location, (type_name, expr)) in fsm.datamodel.iter() {
            let scan_type = self
                .scan_types
                .get(type_name)
                .ok_or(anyhow!("unknown type"))?;
            let var = self
                .cs
                .new_var(pg_id, scan_type.to_owned())
                .expect("program graph exists!");
            vars.insert(location.to_owned(), (var, scan_type.to_owned()));
            // Initialize variable with `expr`, if any, by adding it as effect of `initialize` action.
            if let Some(expr) = expr {
                let expr =
                    self.expression(pg_id, expr, &fsm.interner, &vars, None, &HashMap::new())?;
                // This might fail if `expr` does not typecheck.
                self.cs.add_effect(pg_id, initialize, var, expr)?;
            }
        }
        // Transition initializing datamodel variables.
        // After initializing datamodel, transition to location representing point-of-entry of initial state of State Chart.
        let initial_state = self.cs.new_location(pg_id).expect("program graph exists!");
        self.cs
            .add_transition(pg_id, initial_loc, initialize, initial_state, None)
            .expect("hand-coded args");
        // Map fsm's state ids to corresponding CS's locations.
        let mut states = HashMap::new();
        // Conventionally, the entry-point for a state is a location associated to the id of the state.
        // In particular, the id of the initial state of the fsm has to correspond to the initial location of the program graph.
        states.insert(fsm.initial.to_owned(), initial_state);
        // Var representing the current event and origin pair
        let current_event_and_origin_var = self
            .cs
            .new_var(pg_id, Type::Product(vec![Type::Integer, Type::Integer]))
            .expect("program graph exists!");
        // Var representing the current event
        let current_event_var = self
            .cs
            .new_var(pg_id, Type::Integer)
            .expect("program graph exists!");
        // Implement internal queue
        let int_queue = self.cs.new_channel(Type::Integer, None);
        let dequeue_int = self
            .cs
            .new_communication(pg_id, int_queue, Message::Receive(current_event_var))
            .expect("hand-coded args");
        // Variable that will store origin of last processed event.
        let origin_var = self
            .cs
            .new_var(pg_id, Type::Integer)
            .expect("program graph exists!");
        let set_int_origin = self.cs.new_action(pg_id).expect("program graph exists!");
        self.cs
            .add_effect(
                pg_id,
                set_int_origin,
                origin_var,
                CsExpression::Const(Val::Integer(pg_index)),
            )
            .expect("hand-coded args");
        // Implement external queue
        let dequeue_ext = self
            .cs
            .new_communication(
                pg_id,
                ext_queue,
                Message::Receive(current_event_and_origin_var),
            )
            .expect("hand-coded args");
        // Process external event to assign event and origin values to respective vars
        let process_ext_event = self.cs.new_action(pg_id)?;
        self.cs
            .add_effect(
                pg_id,
                process_ext_event,
                current_event_var,
                CsExpression::Component(
                    0,
                    Box::new(CsExpression::Var(current_event_and_origin_var)),
                ),
            )
            .expect("hand-coded args");
        self.cs
            .add_effect(
                pg_id,
                process_ext_event,
                origin_var,
                CsExpression::Component(
                    1,
                    Box::new(CsExpression::Var(current_event_and_origin_var)),
                ),
            )
            .expect("hand-coded args");
        // Action representing checking the next transition
        let next_transition = self.cs.new_action(pg_id).expect("program graph exists!");

        // Create variables for the storage of the parameters sent by external events.
        let mut params: HashMap<(usize, String), (CsVar, Type)> = HashMap::new();
        for event_builder in self
            .events
            .iter()
            // only consider events that can activate some transition and that some other process is sending.
            .filter(|eb| eb.receivers.contains(&pg_id) && !eb.senders.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
        {
            for (param_name, param_type) in event_builder.params.iter() {
                // Variable where to store parameter.
                let param_var = self
                    .cs
                    .new_var(pg_id, param_type.to_owned())
                    .expect("hand-made input");
                params.insert(
                    (event_builder.index, param_name.to_owned()),
                    (param_var, param_type.to_owned()),
                );
            }
        }

        // Consider each of the fsm's states
        for (state_id, state) in fsm.states.iter() {
            trace!("build state {}", state_id);
            // Each state is modeled by multiple locations connected by transitions
            // A starting location is used as a point-of-entry to the execution of the state.
            let start_loc = if let Some(start_loc) = states.get(state_id) {
                *start_loc
            } else {
                let start_loc = self.cs.new_location(pg_id).expect("program graph exists!");
                states.insert(state_id.to_owned(), start_loc);
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
                    pg_index,
                    int_queue,
                    onentry_loc,
                    &vars,
                    None,
                    &HashMap::new(),
                    &fsm.interner,
                )?;
            }

            // Location where eventless/NULL transitions activate
            let mut null_trans = onentry_loc;
            // Location where internal events are dequeued
            let int_queue_loc = self.cs.new_location(pg_id).expect("program graph exists!");
            // Location where the origin of internal events is set as own.
            let int_origin_loc = self.cs.new_location(pg_id).expect("program graph exists!");
            // Location where external events are dequeued
            let ext_queue_loc = self.cs.new_location(pg_id).expect("program graph exists!");
            // Location where the index/origin of external events are dequeued
            let ext_event_processing_loc =
                self.cs.new_location(pg_id).expect("program graph exists!");
            // Location where eventful transitions activate
            let mut eventful_trans = self.cs.new_location(pg_id).expect("program graph exists!");
            // Transition dequeueing a new internal event and searching for first active eventful transition
            self.cs
                .add_transition(pg_id, int_queue_loc, dequeue_int, int_origin_loc, None)
                .expect("hand-coded args");
            // Transition dequeueing a new internal event and searching for first active eventful transition
            self.cs
                .add_transition(pg_id, int_origin_loc, set_int_origin, eventful_trans, None)
                .expect("hand-coded args");
            // Action denoting checking if internal queue is empty;
            // if so, move to external queue.
            // Notice that one and only one of `int_dequeue` and `empty_int_queue` can be executed at a given time.
            let empty_int_queue = self
                .cs
                .new_communication(pg_id, int_queue, Message::ProbeEmptyQueue)
                .expect("hand-coded args");
            self.cs
                .add_transition(pg_id, int_queue_loc, empty_int_queue, ext_queue_loc, None)
                .expect("hand-coded args");
            // Dequeue a new external event and search for first active named transition.
            self.cs
                .add_transition(
                    pg_id,
                    ext_queue_loc,
                    dequeue_ext,
                    ext_event_processing_loc,
                    None,
                )
                .expect("hand-coded args");
            // Dequeue the origin of the corresponding external event.
            let mut ext_event_processing_param =
                self.cs.new_location(pg_id).expect("program graph exists!");
            self.cs
                .add_transition(
                    pg_id,
                    ext_event_processing_loc,
                    process_ext_event,
                    ext_event_processing_param,
                    None,
                )
                .expect("hand-coded args");
            // Retreive external event's parameters
            // We need to set up the parameter-passing channel for every possible event that could be sent,
            // from any possible other fsm,
            // and for any parameter of the event.
            for event_builder in self
                .events
                .iter()
                .filter(|eb| eb.receivers.contains(&pg_id) && !eb.senders.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
            {
                for ((_event_id, param_name), (param_var, param_type)) in params.iter() {
                    for sender_id in event_builder.senders.iter() {
                        // Channel where to retreive the parameter from (it may or may not exist already).
                        let channel = *self
                            .parameters
                            .entry((
                                *sender_id,
                                pg_id,
                                event_builder.index,
                                param_name.to_owned(),
                            ))
                            .or_insert(self.cs.new_channel(param_type.to_owned(), None));
                        let next = self.cs.new_location(pg_id).expect("program graph exists!");
                        let read_param = self
                            .cs
                            .new_communication(pg_id, channel, Message::Receive(*param_var))
                            .expect("hard-coded input");
                        self.cs
                            .add_transition(
                                pg_id,
                                ext_event_processing_param,
                                read_param,
                                next,
                                None,
                            )
                            .expect("hand-coded args");
                        ext_event_processing_param = next;
                    }
                }
            }

            // Now we can start looking for transitions activated by the external event.
            self.cs
                .add_transition(
                    pg_id,
                    ext_event_processing_param,
                    process_ext_event,
                    eventful_trans,
                    None,
                )
                .expect("hand-coded args");

            // Consider each of the state's transitions.
            for transition in state.transitions.iter() {
                trace!(
                    "build {} transition to {}",
                    transition
                        .event
                        .as_ref()
                        .unwrap_or(&"eventless".to_string()),
                    transition.target
                );
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

                // Set up origin and parameters for conditional/executable content.
                let exec_origin;
                let mut exec_params = HashMap::new();
                if let Some(event_index) = transition.event.as_ref().map(|ev| {
                    self.event_indexes
                        .get(ev)
                        .expect("event must be registered")
                }) {
                    exec_origin = Some(origin_var);
                    for ((_, param_name), (var, var_type)) in params
                        .iter()
                        .filter(|((ev_ix, _), _)| *ev_ix == *event_index)
                    {
                        exec_params.insert(param_name.to_owned(), (*var, var_type.to_owned()));
                    }
                } else {
                    exec_origin = None;
                }
                // Condition activating the transition.
                // It has to be parsed/built as a Boolean expression.
                // Could fail if `expr` is invalid.
                let cond: Option<CsExpression> = transition
                    .cond
                    .as_ref()
                    .map(|cond| {
                        self.expression(
                            pg_id,
                            cond,
                            &fsm.interner,
                            &vars,
                            exec_origin,
                            &exec_params,
                        )
                    })
                    .transpose()?;
                // Guard for transition.
                // Has to be defined depending on the type of transition, etc...
                let guard;
                // Proceed on whether the transition is eventless or activated by event.
                if let Some(event) = &transition.event {
                    // Create event, if it does not exist already.
                    let event_idx = self.event_index(event) as Integer;
                    // Check if the current event (internal or external) corresponds to the event activating the transition.
                    let event_match = CsExpression::Equal(Box::new((
                        CsExpression::Var(current_event_var),
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
                // First execute the executable content of the state's `on_exit` tag,
                // then that of the `transition` tag.
                for exec in state.on_exit.iter().chain(transition.effects.iter()) {
                    exec_trans_loc = self.add_executable(
                        exec,
                        pg_id,
                        pg_index,
                        int_queue,
                        exec_trans_loc,
                        &vars,
                        exec_origin,
                        &exec_params,
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
        vars: &HashMap<String, (CsVar, Type)>,
        origin: Option<CsVar>,
        params: &HashMap<String, (CsVar, Type)>,
        interner: &boa_interner::Interner,
    ) -> Result<CsLocation, anyhow::Error> {
        match executable {
            Executable::Raise { event } => {
                // Create event, if it does not exist already.
                let event_idx = self.event_index(event);
                let raise = self.cs.new_communication(
                    pg_id,
                    int_queue,
                    Message::Send(CsExpression::Const(Val::Integer(event_idx as Integer))),
                )?;
                let next_loc = self.cs.new_location(pg_id)?;
                // queue the internal event
                self.cs.add_transition(pg_id, loc, raise, next_loc, None)?;
                Ok(next_loc)
            }
            Executable::Send {
                event,
                target,
                params: send_params,
            } => match target {
                Target::Id(target) => {
                    let target_builder = self.fsm_builder(target);
                    let target_id = target_builder.pg_id;
                    let target_ext_queue = target_builder.ext_queue;
                    // Create event, if it does not exist already.
                    let event_idx = self.event_index(event);
                    let send_event = self.cs.new_communication(
                        pg_id,
                        target_ext_queue,
                        Message::Send(CsExpression::Const(Val::Tuple(vec![
                            Val::Integer(event_idx as Integer),
                            Val::Integer(pg_idx),
                        ]))),
                    )?;

                    // Send event and event origin before moving on to next location.
                    let mut next_loc = self.cs.new_location(pg_id)?;
                    self.cs
                        .add_transition(pg_id, loc, send_event, next_loc, None)?;

                    // Pass parameters.
                    for param in send_params {
                        // Updates next location.
                        next_loc = self.send_param(
                            pg_id, target_id, param, event_idx, next_loc, vars, interner,
                        )?;
                    }

                    Ok(next_loc)
                }
                Target::Expr(targetexpr) => {
                    let targetexpr =
                        self.expression(pg_id, targetexpr, interner, vars, origin, params)?;
                    let event_idx = self.event_index(event);
                    // Location representing having sent the event to the correct target after evaluating expression.
                    let done_loc = self.cs.new_location(pg_id).expect("PG exists");
                    for &target_id in self.events[event_idx].receivers.clone().iter() {
                        // FIXME TODO: there should be an indexing to avoid search
                        let (_target_name, target_builder) = self
                            .fsm_builders
                            .iter()
                            .find(|(_, b)| b.pg_id == target_id)
                            .expect("fsm has to be here");
                        let target_index = target_builder.index;
                        let target_ext_queue = target_builder.ext_queue;
                        let send_event = self.cs.new_communication(
                            pg_id,
                            target_ext_queue,
                            Message::Send(CsExpression::Const(Val::Tuple(vec![
                                Val::Integer(event_idx as Integer),
                                Val::Integer(pg_idx),
                            ]))),
                        )?;

                        // Send event and event origin before moving on to next location.
                        let mut next_loc = self.cs.new_location(pg_id)?;
                        self.cs.add_transition(
                            pg_id,
                            loc,
                            send_event,
                            next_loc,
                            Some(CsExpression::Equal(Box::new((
                                CsExpression::Const(Val::Integer(target_index as Integer)),
                                targetexpr.to_owned(),
                            )))),
                        )?;

                        // Pass parameters.
                        for param in send_params {
                            // Updates next location.
                            next_loc = self.send_param(
                                pg_id, target_id, param, event_idx, next_loc, vars, interner,
                            )?;
                        }
                        // Once sending event and args done, get to exit-point
                        self.cs
                            .add_transition(pg_id, next_loc, send_event, done_loc, None)
                            .expect("hand-made args");
                    }

                    // Return exit point
                    Ok(done_loc)
                }
            },
            Executable::Assign { location, expr } => {
                // Add a transition that perform the assignment via the effect of the `assign` action.
                let expr = self.expression(pg_id, expr, interner, &vars, origin, params)?;
                let (var, _scan_type) = vars.get(location).ok_or(anyhow!("undefined variable"))?;
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
        event_idx: usize,
        next_loc: CsLocation,
        vars: &HashMap<String, (CsVar, Type)>,
        interner: &boa_interner::Interner,
    ) -> Result<CsLocation, anyhow::Error> {
        // Get param type.
        let scan_type = self
            .scan_types
            .get(&param.omg_type)
            .cloned()
            .ok_or(anyhow!("undefined type"))?;
        // Build expression from ECMAScript expression.
        let expr = self.expression(pg_id, &param.expr, interner, &vars, None, &HashMap::new())?;
        // Retreive or create channel for parameter passing.
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
        vars: &HashMap<String, (CsVar, Type)>,
        origin: Option<CsVar>,
        params: &HashMap<String, (CsVar, Type)>,
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
                    ident => self
                        .enums
                        .get(ident)
                        .map(|i| Expression::Const(Val::Integer(*i)))
                        .or_else(|| vars.get(ident).map(|(var, _)| CsExpression::Var(*var)))
                        .ok_or(anyhow!("unknown identifier"))?,
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
                                    if let Some((param_var, var_type)) = params.get(var_ident) {
                                        match var_type {
                                            Type::Unit => todo!(),
                                            Type::Boolean => CsExpression::Var(*param_var),
                                            Type::Integer => CsExpression::Var(*param_var),
                                            Type::Product(_) => todo!(),
                                        }
                                    } else {
                                        return Err(anyhow!(
                                            "no parameter `{ident}` found among: {params:#?}"
                                        ));
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
                            self.expression(pg_id, bin.lhs(), interner, vars, origin, params)?;
                        let rhs =
                            self.expression(pg_id, bin.rhs(), interner, vars, origin, params)?;
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
                            self.expression(pg_id, bin.lhs(), interner, vars, origin, params)?;
                        let rhs =
                            self.expression(pg_id, bin.rhs(), interner, vars, origin, params)?;
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
            .fsm_builders
            .iter()
            .map(|(name, id)| (id.pg_id, name.to_owned()))
            .collect();
        CsModel {
            cs: self.cs.build(),
            fsm_names,
        }
    }
}
