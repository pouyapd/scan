use std::collections::HashMap;
use std::io::BufRead;
use std::str;

use anyhow::anyhow;
use log::{error, info, trace, warn};
use quick_xml::events::attributes::{AttrError, Attribute};
use quick_xml::{events, events::Event, Reader};

use super::vocabulary::*;
use crate::{ParserError, ParserErrorType};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScxmlTag {
    State(String),
    Transition,
    Scxml,
    Datamodel,
    OnEntry,
    OnExit,
    Send,
    // Data,
}

impl From<ScxmlTag> for &'static str {
    fn from(value: ScxmlTag) -> Self {
        match value {
            ScxmlTag::State(_) => TAG_STATE,
            ScxmlTag::Transition => TAG_TRANSITION,
            ScxmlTag::Scxml => TAG_SCXML,
            ScxmlTag::Datamodel => TAG_DATAMODEL,
            ScxmlTag::OnEntry => TAG_ONENTRY,
            ScxmlTag::OnExit => TAG_ONEXIT,
            ScxmlTag::Send => TAG_SEND,
            // ScxmlTag::Data => TAG_DATA,
        }
    }
}

impl ScxmlTag {
    pub fn is_executable(&self) -> bool {
        matches!(
            self,
            ScxmlTag::OnEntry | ScxmlTag::OnExit | ScxmlTag::Transition
        )
    }
}

#[derive(Debug, Clone)]
pub struct State {
    pub(crate) id: String,
    pub(crate) transitions: Vec<Transition>,
    pub(crate) on_entry: Vec<Executable>,
    pub(crate) on_exit: Vec<Executable>,
}

#[derive(Debug, Clone)]
pub struct Transition {
    pub(crate) event: Option<String>,
    pub(crate) target: String,
    pub(crate) cond: Option<String>,
    pub(crate) effects: Vec<Executable>,
}

#[derive(Debug, Clone)]
pub enum Executable {
    Raise {
        event: String,
    },
    Send {
        event: String,
        target: String,
        params: Vec<Param>,
    },
}

#[derive(Debug, Clone)]
pub struct Param {
    pub(crate) name: String,
    pub(crate) location: String,
}

#[derive(Debug, Clone)]
pub struct Fsm {
    pub(crate) id: String,
    pub(crate) initial: String,
    pub(crate) datamodel: HashMap<String, ()>,
    pub(crate) states: HashMap<String, State>,
}

impl Fsm {
    pub(super) fn parse_skill<R: BufRead>(reader: &mut Reader<R>) -> anyhow::Result<Self> {
        let mut fsm = Fsm {
            id: String::new(),
            initial: String::new(),
            datamodel: HashMap::new(),
            states: HashMap::new(),
        };
        let mut buf = Vec::new();
        let mut stack: Vec<ScxmlTag> = Vec::new();
        info!("parsing fsm");
        loop {
            match reader.read_event_into(&mut buf)? {
                Event::Start(tag) => {
                    let tag_name = tag.name();
                    let tag_name = str::from_utf8(tag_name.as_ref())?;
                    trace!("'{tag_name}' open tag");
                    match tag_name {
                        TAG_SCXML if stack.is_empty() => {
                            fsm.parse_scxml(tag, reader)?;
                            stack.push(ScxmlTag::Scxml);
                        }
                        TAG_DATAMODEL
                            if stack
                                .last()
                                .is_some_and(|tag| matches!(*tag, ScxmlTag::Scxml)) =>
                        {
                            stack.push(ScxmlTag::Datamodel);
                        }
                        TAG_STATE
                            if stack
                                .last()
                                .is_some_and(|tag| matches!(*tag, ScxmlTag::Scxml)) =>
                        {
                            let id = fsm.parse_state(tag, reader)?;
                            stack.push(ScxmlTag::State(id));
                        }
                        TAG_TRANSITION
                            if stack
                                .last()
                                .is_some_and(|tag| matches!(*tag, ScxmlTag::State(_))) =>
                        {
                            fsm.parse_transition(tag, reader, &stack)?;
                            stack.push(ScxmlTag::Transition);
                        }
                        TAG_ONENTRY
                            if stack
                                .last()
                                .is_some_and(|tag| matches!(*tag, ScxmlTag::State(_))) =>
                        {
                            stack.push(ScxmlTag::OnEntry);
                        }
                        TAG_ONEXIT
                            if stack
                                .last()
                                .is_some_and(|tag| matches!(*tag, ScxmlTag::State(_))) =>
                        {
                            stack.push(ScxmlTag::OnExit);
                        }
                        // Unknown tag: skip till maching end tag
                        _ => {
                            warn!("unknown or unexpected tag {tag_name}, skipping");
                            reader.read_to_end_into(tag.to_end().into_owned().name(), &mut buf)?;
                        }
                    }
                }
                Event::End(tag) => {
                    let tag_name = tag.name();
                    let tag_name = str::from_utf8(tag_name.as_ref())?;
                    if stack.pop().is_some_and(|tag| <&str>::from(tag) == tag_name) {
                        trace!("'{tag_name}' end tag");
                    } else {
                        error!("unexpected end tag {tag_name}");
                        return Err(anyhow::Error::new(ParserError(
                            reader.buffer_position(),
                            ParserErrorType::UnexpectedEndTag(tag_name.to_string()),
                        )));
                    }
                }
                Event::Empty(tag) => {
                    let tag_name = tag.name();
                    let tag_name = str::from_utf8(tag_name.as_ref())?;
                    trace!("'{tag_name}' empty tag");
                    // let tag_name = ConvinceTag::from(tag_name.as_str());
                    match tag_name {
                        TAG_DATA
                            if stack
                                .last()
                                .is_some_and(|tag| matches!(*tag, ScxmlTag::Datamodel)) =>
                        {
                            // TODO: implement 'data' tag
                        }
                        TAG_TRANSITION
                            if stack
                                .last()
                                .is_some_and(|tag| matches!(*tag, ScxmlTag::State(_))) =>
                        {
                            fsm.parse_transition(tag, reader, &stack)?;
                        }
                        // we `rev()` the iterator only because we expect the relevant tag to be towards the end of the stack
                        TAG_RAISE if stack.iter().rev().any(|tag| tag.is_executable()) => {
                            fsm.parse_raise(tag, reader, &stack)?;
                        }
                        TAG_SEND if stack.iter().rev().any(|tag| tag.is_executable()) => {
                            fsm.parse_send(tag, reader, &stack)?;
                        }
                        TAG_PARAM
                            if stack.iter().rev().any(|tag| matches!(*tag, ScxmlTag::Send)) =>
                        {
                            fsm.parse_param(tag, reader, &stack)?;
                        }
                        // Unknown tag: skip till maching end tag
                        _ => {
                            warn!("unknown or unexpected tag {tag_name:?}, skipping");
                            continue;
                        }
                    }
                }
                Event::Text(_) => continue,
                Event::Comment(_) => continue,
                Event::CData(_) => todo!(),
                Event::Decl(_) => todo!(), // parser.parse_xml_declaration(tag)?,
                Event::PI(_) => todo!(),
                Event::DocType(_) => todo!(),
                // exits the loop when reaching end of file
                Event::Eof => {
                    info!("parsing completed");
                    if !stack.is_empty() {
                        return Err(anyhow!(ParserError(
                            reader.buffer_position(),
                            ParserErrorType::UnclosedTags,
                        )));
                    }
                    break;
                }
            }
            // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
            buf.clear();
        }
        Ok(fsm)
    }

    fn parse_scxml<R: BufRead>(
        &mut self,
        tag: events::BytesStart<'_>,
        _reader: &mut Reader<R>,
    ) -> anyhow::Result<()> {
        for attr in tag
            .attributes()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_NAME => {
                    self.id = String::from_utf8(attr.value.into_owned())?;
                }
                ATTR_INITIAL => {
                    self.initial = String::from_utf8(attr.value.into_owned())?;
                }
                key => {
                    error!("found unknown attribute {key} in {TAG_STATE}, ignoring");
                    continue;
                }
            }
        }
        Ok(())
    }

    fn parse_state<R: BufRead>(
        &mut self,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
    ) -> anyhow::Result<String> {
        let mut id: Option<String> = None;
        for attr in tag
            .attributes()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_ID => {
                    id = Some(String::from_utf8(attr.value.into_owned())?);
                }
                key => {
                    error!("found unknown attribute {key} in {TAG_STATE}");
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        let id = id.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_ID.to_string())
        )))?;
        // Check if it is the initial state
        if self.initial.is_empty() {
            self.initial = id.to_owned();
        }
        let state = State {
            id: id.to_owned(),
            transitions: Vec::new(),
            on_entry: Vec::new(),
            on_exit: Vec::new(),
        };
        // Here it should be checked that no component was already in the list under the same name
        self.states.insert(id.to_owned(), state);
        Ok(id)
    }

    fn parse_transition<R: BufRead>(
        &mut self,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
        stack: &[ScxmlTag],
    ) -> anyhow::Result<()> {
        let state: &str = stack
            .iter()
            .rev()
            .find_map(|tag| {
                if let ScxmlTag::State(state) = tag {
                    Some(state)
                } else {
                    None
                }
            })
            .ok_or_else(|| ParserError(reader.buffer_position(), ParserErrorType::NotAState))?;
        let mut event: Option<String> = None;
        let mut target: Option<String> = None;
        let mut cond: Option<String> = None;
        for attr in tag
            .attributes()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_EVENT => {
                    event = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_TARGET => {
                    target = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_COND => {
                    cond = Some(String::from_utf8(attr.value.into_owned())?);
                }
                key => {
                    error!("found unknown attribute {key} in {TAG_TRANSITION}");
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        let target = target.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_TARGET.to_string())
        )))?;
        let transition = Transition {
            event,
            target,
            cond,
            effects: Vec::new(),
        };
        // Need to know current state
        self.states
            .get_mut(state)
            .expect("the state tag has already been processed")
            .transitions
            .push(transition);
        Ok(())
    }

    fn parse_raise<R: BufRead>(
        &mut self,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
        stack: &[ScxmlTag],
    ) -> anyhow::Result<()> {
        let mut event: Option<String> = None;
        for attr in tag
            .attributes()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_EVENT => {
                    event = Some(String::from_utf8(attr.value.into_owned())?);
                }
                key => {
                    error!("found unknown attribute {key} in {TAG_TRANSITION}");
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        let event = event.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_EVENT.to_string())
        )))?;
        let executable = Executable::Raise { event };
        let state_id: &str = stack
            .iter()
            .rev()
            .find_map(|tag| {
                if let ScxmlTag::State(state) = tag {
                    Some(state)
                } else {
                    None
                }
            })
            .ok_or_else(|| ParserError(reader.buffer_position(), ParserErrorType::NotAState))?;
        let state = self
            .states
            .get_mut(state_id)
            .expect("State in stack has to exist");
        match stack
            .iter()
            .rfind(|tag| tag.is_executable())
            .expect("there must be an executable tag")
        {
            ScxmlTag::OnEntry => {
                state.on_entry.push(executable);
            }
            ScxmlTag::OnExit => {
                state.on_exit.push(executable);
            }
            ScxmlTag::Transition => {
                state
                    .transitions
                    .last_mut()
                    .expect("inside a `Transition` tag")
                    .effects
                    .push(executable);
            }
            _ => panic!("non executable tag"),
        }
        Ok(())
    }

    fn parse_send<R: BufRead>(
        &mut self,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
        stack: &[ScxmlTag],
    ) -> anyhow::Result<()> {
        let mut event: Option<String> = None;
        let mut target: Option<String> = None;
        for attr in tag
            .attributes()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_EVENT => {
                    event = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_TARGET => {
                    target = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_TARGETEXPR => {
                    // TODO: implement target expressions
                    return Ok(());
                }
                key => {
                    error!("found unknown attribute {key} in {TAG_TRANSITION}");
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        let event = event.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_EVENT.to_string())
        )))?;
        let target = target.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_TARGET.to_string())
        )))?;
        let executable = Executable::Send {
            event,
            target,
            params: Vec::new(),
        };
        let state_id: &str = stack
            .iter()
            .rev()
            .find_map(|tag| {
                if let ScxmlTag::State(state) = tag {
                    Some(state)
                } else {
                    None
                }
            })
            .ok_or_else(|| ParserError(reader.buffer_position(), ParserErrorType::NotAState))?;
        let state = self
            .states
            .get_mut(state_id)
            .expect("State in stack has to exist");
        match stack
            .iter()
            .rfind(|tag| tag.is_executable())
            .expect("there must be an executable tag")
        {
            ScxmlTag::OnEntry => {
                state.on_entry.push(executable);
            }
            ScxmlTag::OnExit => {
                state.on_exit.push(executable);
            }
            ScxmlTag::Transition => {
                state
                    .transitions
                    .last_mut()
                    .expect("inside a `Transition` tag")
                    .effects
                    .push(executable);
            }
            _ => panic!("non executable tag"),
        }
        Ok(())
    }

    fn parse_param<R: BufRead>(
        &mut self,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
        stack: &[ScxmlTag],
    ) -> anyhow::Result<()> {
        let mut name: Option<String> = None;
        let mut location: Option<String> = None;
        for attr in tag
            .attributes()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_NAME => {
                    name = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_LOCATION => {
                    location = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_EXPR => {
                    // TODO: implement target expressions
                    return Ok(());
                }
                key => {
                    error!("found unknown attribute {key} in {TAG_TRANSITION}");
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        let name = name.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_EVENT.to_string())
        )))?;
        let location = location.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_TARGET.to_string())
        )))?;
        let param = Param { name, location };

        // Find which `State` is being parsed.
        let state_id: &str = stack
            .iter()
            .rev()
            .find_map(|tag| {
                if let ScxmlTag::State(state) = tag {
                    Some(state)
                } else {
                    None
                }
            })
            .ok_or_else(|| ParserError(reader.buffer_position(), ParserErrorType::NotAState))?;
        let state = self
            .states
            .get_mut(state_id)
            .expect("State in stack has to exist");

        // Find in which executable element the `Send` (the `Param` belongs to) is.
        // The `Send` must be the last `Executable` being parsed.
        // Then, push the `Param`.
        // TODO: Handle errors.
        match stack
            .iter()
            .rfind(|tag| tag.is_executable())
            .expect("there must be an executable tag")
        {
            ScxmlTag::OnEntry => {
                if let Some(Executable::Send {
                    event: _,
                    target: _,
                    params,
                }) = state.on_entry.last_mut()
                {
                    params.push(param);
                }
            }
            ScxmlTag::OnExit => {
                if let Some(Executable::Send {
                    event: _,
                    target: _,
                    params,
                }) = state.on_exit.last_mut()
                {
                    params.push(param);
                }
            }
            ScxmlTag::Transition => {
                if let Some(Executable::Send {
                    event: _,
                    target: _,
                    params,
                }) = state
                    .transitions
                    .last_mut()
                    .expect("inside a `Transition` tag")
                    .effects
                    .last_mut()
                {
                    params.push(param);
                }
            }
            _ => panic!("non executable tag"),
        }
        Ok(())
    }
}
