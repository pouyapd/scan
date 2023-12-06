use std::{collections::HashMap, error::Error, fmt, rc::Rc};

use crate::{
    Action, Effect, Formula, Location, PgError, ProgramGraph, ProgramGraphBuilder, Val, Var,
    VarType,
};

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct PgId(usize);

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Channel(usize);

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct CsLocation(PgId, Location);

// Use of "Newtype" pattern to define different types of indexes.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct CsAction(PgId, Action);

#[derive(Debug, Clone)]
pub enum Message {
    Send(Effect),
    Receive(Var),
}

#[derive(Debug, Clone, Copy)]
pub enum CsErr {
    ProgramGraph(PgId, PgError),
    MissingPg(PgId),
    OutOfCapacity(Channel),
    Empty(Channel),
    NoCommunication((PgId, Action)),
}

impl fmt::Display for CsErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CsErr::ProgramGraph(pg_id, _) => write!(f, "error from program graph {:?}", pg_id),
            CsErr::MissingPg(pg_id) => {
                write!(
                    f,
                    "program graph {:?} does not belong to the channel system",
                    pg_id
                )
            }
            CsErr::OutOfCapacity(channel) => write!(f, "channel {:?} is at full capacity", channel),
            CsErr::Empty(channel) => write!(f, "channel {:?} is empty", channel),
            CsErr::NoCommunication(comm) => {
                write!(f, "communication {:?} has not been defined", comm)
            }
        }
    }
}

impl Error for CsErr {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            CsErr::ProgramGraph(_, err) => Some(err),
            _ => None,
        }
    }
}

pub struct ChannelSystemBuilder {
    program_graphs: Vec<ProgramGraphBuilder>,
    channels: Vec<(VarType, usize)>,
    communications: HashMap<(PgId, Action), (Channel, Message)>,
}

impl Default for ChannelSystemBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelSystemBuilder {
    pub fn new() -> Self {
        Self {
            program_graphs: Vec::new(),
            channels: Vec::new(),
            communications: HashMap::new(),
        }
    }

    pub fn new_program_graph(&mut self) -> PgId {
        let pg_id = PgId(self.program_graphs.len());
        self.program_graphs.push(ProgramGraphBuilder::new());
        pg_id
    }

    pub fn new_var(&mut self, pg_id: PgId, var_type: VarType) -> Result<Var, CsErr> {
        self.program_graphs
            .get_mut(pg_id.0)
            .ok_or(CsErr::MissingPg(pg_id))
            .map(|pg| pg.new_var(var_type))
    }

    pub fn new_action(&mut self, pg_id: PgId) -> Result<Action, CsErr> {
        self.program_graphs
            .get_mut(pg_id.0)
            .ok_or(CsErr::MissingPg(pg_id))
            .map(|pg| pg.new_action())
    }

    pub fn add_effect(
        &mut self,
        pg_id: PgId,
        action: Action,
        var: Var,
        effect: Effect,
    ) -> Result<(), CsErr> {
        self.program_graphs
            .get_mut(pg_id.0)
            .ok_or(CsErr::MissingPg(pg_id))
            .and_then(|pg| {
                pg.add_effect(action, var, effect)
                    .map_err(|err| CsErr::ProgramGraph(pg_id, err))
            })
    }

    pub fn new_location(&mut self, pg_id: PgId) -> Result<Location, CsErr> {
        self.program_graphs
            .get_mut(pg_id.0)
            .ok_or(CsErr::MissingPg(pg_id))
            .map(|pg| pg.new_location())
    }

    pub fn add_transition(
        &mut self,
        pg_id: PgId,
        pre: Location,
        action: Action,
        post: Location,
        guard: Formula,
    ) -> Result<(), CsErr> {
        self.program_graphs
            .get_mut(pg_id.0)
            .ok_or(CsErr::MissingPg(pg_id))
            .and_then(|pg| {
                pg.add_transition(pre, action, post, guard)
                    .map_err(|err| CsErr::ProgramGraph(pg_id, err))
            })
    }

    pub fn new_channel(&mut self, var_type: VarType, capacity: usize) -> Channel {
        let channel = Channel(self.channels.len());
        self.channels.push((var_type, capacity));
        channel
    }

    pub fn new_communication(
        &mut self,
        pg_id: PgId,
        channel: Channel,
        pre: Location,
        message: Message,
        post: Location,
        guard: Formula,
    ) -> Result<Action, CsErr> {
        // This can only fail if 'pg_id' is not a valid id
        let action = self.new_action(pg_id)?;
        // If this fails, there will be a useless action left in the pg:
        // as it is not returned, the user never sees it.
        // Is it worth removing?
        self.add_transition(pg_id, pre, action, post, guard)?;
        self.communications
            .insert((pg_id, action), (channel, message));
        Ok(action)
    }

    pub fn build(mut self) -> ChannelSystem {
        self.program_graphs.shrink_to_fit();
        self.channels.shrink_to_fit();
        self.communications.shrink_to_fit();
        ChannelSystem {
            program_graphs: self
                .program_graphs
                .into_iter()
                .map(|builder| builder.build())
                .collect(),
            communications: Rc::new(self.communications),
            message_queue: vec![Vec::default(); self.channels.len()],
            channels: Rc::new(self.channels),
        }
    }
}

pub struct ChannelSystem {
    program_graphs: Vec<ProgramGraph>,
    channels: Rc<Vec<(VarType, usize)>>,
    communications: Rc<HashMap<(PgId, Action), (Channel, Message)>>,
    message_queue: Vec<Vec<Val>>,
}

impl ChannelSystem {
    // Is this function optimized? Does it unnecessarily copy data?
    pub fn possible_transitions(&self) -> Vec<(PgId, Action, Location)> {
        self.program_graphs
            .iter()
            .enumerate()
            .flat_map(|(id, pg)| {
                let pg_id = PgId(id);
                pg.possible_transitions()
                    .iter()
                    .filter_map(|(action, post)| {
                        if self.communications.contains_key(&(pg_id, *action))
                            && self.check_communication(pg_id, *action).is_err()
                        {
                            None
                        } else {
                            Some((pg_id, *action, *post))
                        }
                    })
                    .collect::<Vec<(PgId, Action, Location)>>()
            })
            .collect::<Vec<(PgId, Action, Location)>>()
    }

    fn check_communication(&self, pg_id: PgId, action: Action) -> Result<(), CsErr> {
        if let Some((channel, message)) = self.communications.get(&(pg_id, action)) {
            let (_, capacity) = self
                .channels
                .get(channel.0)
                .expect("communication has been verified before");
            let queue = self
                .message_queue
                .get(channel.0)
                .expect("communication has been verified before");
            match message {
                Message::Send(_) => {
                    let len = queue.len();
                    // Channel capacity must never be exeeded!
                    assert!(len <= *capacity);
                    if len == *capacity {
                        Err(CsErr::OutOfCapacity(*channel))
                    } else {
                        Ok(())
                    }
                }
                Message::Receive(_) => {
                    if queue.is_empty() {
                        Err(CsErr::Empty(*channel))
                    } else {
                        Ok(())
                    }
                }
            }
        } else {
            Err(CsErr::NoCommunication((pg_id, action)))
        }
    }

    pub fn transition(&mut self, pg_id: PgId, action: Action, post: Location) -> Result<(), CsErr> {
        // If action is a communication, check it is legal
        if self.communications.contains_key(&(pg_id, action)) {
            self.check_communication(pg_id, action)?;
        }
        // Transition the program graph
        let pg = self
            .program_graphs
            .get_mut(pg_id.0)
            .ok_or(CsErr::MissingPg(pg_id))?;
        pg.transition(action, post)
            .map_err(|err| CsErr::ProgramGraph(pg_id, err))?;
        // If the action is a communication, send/receive the message
        if let Some((channel, message)) = self.communications.get(&(pg_id, action)) {
            let queue = self
                .message_queue
                .get_mut(channel.0)
                .expect("communication has been verified before");
            match message {
                Message::Send(effect) => {
                    let val = pg.eval(effect);
                    queue.push(val);
                }
                Message::Receive(var) => {
                    let val = queue.pop().expect("communication has been verified before");
                    pg.assign(*var, val)
                        .expect("communication has been verified before");
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder() {
        let _cs = ChannelSystemBuilder::new();
    }

    #[test]
    fn new_pg() {
        let mut cs = ChannelSystemBuilder::new();
        let _pg = cs.new_program_graph();
    }

    #[test]
    fn new_action() -> Result<(), CsErr> {
        let mut cs = ChannelSystemBuilder::new();
        let pg = cs.new_program_graph();
        let _action = cs.new_action(pg)?;
        Ok(())
    }
}
