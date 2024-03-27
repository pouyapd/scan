use core::panic;
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
            // ScxmlTag::Data => TAG_DATA,
        }
    }
}

impl ScxmlTag {
    pub fn is_executable(&self) -> bool {
        matches!(self, ScxmlTag::OnEntry | ScxmlTag::OnExit)
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
}

#[derive(Debug, Clone)]
pub enum Executable {
    Raise { event: String },
    Send { event: String, target: String },
}

#[derive(Debug, Clone)]
pub struct Fsm {
    pub(crate) initial: String,
    pub(crate) datamodel: HashMap<String, ()>,
    pub(crate) states: HashMap<String, State>,
}

impl Fsm {
    pub(super) fn parse_skill<R: BufRead>(reader: &mut Reader<R>) -> anyhow::Result<Self> {
        let mut fsm = Fsm {
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
                                .is_some_and(|tag| matches!(*tag, ScxmlTag::Datamodel)) => {}
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
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
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
            .into_iter()
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
            .into_iter()
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
            .into_iter()
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
            .into_iter()
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
        let executable = Executable::Send { event, target };
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
            _ => panic!("non executable tag"),
        }
        Ok(())
    }
}

// impl Parser {
//     pub(super) fn parse_skill<R: BufRead>(&mut self, reader: &mut Reader<R>) -> anyhow::Result<()> {
//         let mut buf = Vec::new();
//         info!("begin parsing");
//         loop {
//             info!("processing new event");
//             match reader.read_event_into(&mut buf).map_err(|err| {
//                 ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
//             })? {
//                 Event::Start(tag) => {
//                     match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                         ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                     })? {
//                         Self::SPECIFICATION => {
//                             // info!("parsing CONVINCE XML document");
//                             // self.parse_convince_xml(&tag, reader)?;
//                         }
//                         Self::SCXML => {
//                             info!("found new scxml open tag");
//                             self.parse_scxml(&tag, reader)?;
//                         }
//                         // Unknown tag: skip till maching end tag
//                         tag_name => {
//                             warn!("found unknown tag {tag_name}, skipping");
//                             reader
//                                 .read_to_end_into(tag.to_end().into_owned().name(), &mut buf)
//                                 .map_err(|err| {
//                                     ParserError(
//                                         reader.buffer_position(),
//                                         ParserErrorType::Reader(err),
//                                     )
//                                 })?;
//                         }
//                     }
//                 }
//                 // exits the loop when reaching end of file
//                 Event::Eof => {
//                     info!("parsing completed");
//                     return Ok(());
//                 }
//                 Event::End(tag) => {
//                     let name = str::from_utf8(tag.name().as_ref())
//                         .map_err(|err| {
//                             ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                         })?
//                         .to_string();
//                     error!("unexpected end tag {name}");
//                     return Err(anyhow::anyhow!(ParserError(
//                         reader.buffer_position(),
//                         ParserErrorType::UnexpectedEndTag(name),
//                     )));
//                 }
//                 Event::Empty(_) => todo!(),
//                 Event::Text(_) => warn!("skipping text"),
//                 Event::Comment(_) => warn!("skipping comment"),
//                 Event::CData(_) => todo!(),
//                 Event::Decl(tag) => self.parse_xml_declaration(tag)?,
//                 Event::PI(_) => todo!(),
//                 Event::DocType(_) => todo!(),
//             }
//             // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
//             buf.clear();
//         }
//     }

//     fn parse_scxml<R: BufRead>(
//         &mut self,
//         tag: &events::BytesStart,
//         reader: &mut Reader<R>,
//     ) -> Result<(), ParserError> {
//         // let mut initial = None;
//         // let mut name = None;
//         let mut xmlns = None;
//         let mut version = None;
//         let mut datamodel = None;
//         let mut binding = None;
//         let pg_id = self.model.new_program_graph();
//         for attr in tag
//             .attributes()
//             .into_iter()
//             .collect::<Result<Vec<Attribute>, AttrError>>()
//             .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Attr(err)))?
//         {
//             // match attr.key.as_ref() {
//             match str::from_utf8(attr.key.as_ref())
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Utf8(err)))?
//             {
//                 Self::INITIAL => {
//                     // initial = Some(attr.value.as_ref());
//                     let cs_id = self.model.initial_location(pg_id).expect("pg_id exists");
//                     let id = str::from_utf8(attr.value.as_ref()).map_err(|err| {
//                         ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                     })?;
//                     self.states.insert(id.to_owned(), cs_id);
//                 }
//                 Self::VERSION => version = Some(attr.value.as_ref()),
//                 Self::XMLNS => xmlns = Some(attr.value.as_ref()),
//                 Self::DATAMODEL => datamodel = Some(attr.value.as_ref()),
//                 Self::BINDING => binding = Some(attr.value.as_ref()),
//                 Self::NAME => {
//                     let name = str::from_utf8(attr.value.as_ref()).map_err(|err| {
//                         ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                     })?;
//                     self.skills.insert(name.to_owned(), pg_id);
//                 }
//                 key => {
//                     return Err(ParserError(
//                         reader.buffer_position(),
//                         ParserErrorType::UnknownKey(key.to_owned()),
//                     ));
//                 }
//             }
//         }
//         let mut buf = Vec::new();
//         loop {
//             match reader.read_event_into(&mut buf).map_err(|err| {
//                 ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
//             })? {
//                 Event::Empty(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                 })? {
//                     _ => continue,
//                 },
//                 Event::Start(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                 })? {
//                     Self::STATE => {
//                         self.parse_state(tag, reader, pg_id)?;
//                     }
//                     Self::DATAMODEL => {
//                         self.parse_datamodel(&tag, reader, pg_id)?;
//                     }
//                     // Unknown tag: skip till maching end tag
//                     tag_name => {
//                         warn!("unknown tag {tag_name}, skipping");
//                         reader
//                             .read_to_end_into(tag.to_end().into_owned().name(), &mut buf)
//                             .map_err(|err| {
//                                 ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
//                             })?;
//                     }
//                 },
//                 Event::End(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                 })? {
//                     // Self::STATE => return Ok(()),
//                     Self::SCXML => {
//                         info!("done parsing scxml");
//                         return Ok(());
//                     }
//                     name => {
//                         error!("unexpected end tag {name}");
//                         return Err(ParserError(
//                             reader.buffer_position(),
//                             ParserErrorType::UnexpectedEndTag(name.to_string()),
//                         ));
//                     }
//                 },
//                 // exits the loop when reaching end of file
//                 Event::Eof => todo!(),
//                 Event::Text(_) => warn!("skipping text"),
//                 Event::Comment(_) => warn!("skipping comment"),
//                 Event::CData(_) => todo!(),
//                 Event::Decl(_) => todo!(),
//                 Event::PI(_) => todo!(),
//                 Event::DocType(_) => todo!(),
//             }
//             // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
//             buf.clear();
//         }
//     }

//     fn parse_state<R: BufRead>(
//         &mut self,
//         tag: events::BytesStart,
//         reader: &mut Reader<R>,
//         pg_id: PgId,
//     ) -> Result<(), ParserError> {
//         let mut location = None;
//         for attr in tag.attributes() {
//             let attr = attr
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Attr(err)))?;
//             match str::from_utf8(attr.key.as_ref())
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Utf8(err)))?
//             {
//                 Self::ID => {
//                     let state_id = str::from_utf8(attr.value.as_ref()).map_err(|err| {
//                         ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                     })?;
//                     if let Some(state) = self.states.get(state_id) {
//                         location = Some(*state);
//                     } else {
//                         let state = self.model.new_location(pg_id).map_err(|err| {
//                             ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
//                         })?;
//                         location = Some(state);
//                         let previous = self.states.insert(state_id.to_owned(), state);
//                         assert!(previous.is_none(), "states did not contain the key");
//                     }
//                 }
//                 name => warn!("unknown attribute {name}, ignoring"),
//             }
//         }
//         if location.is_none() {
//             location =
//                 Some(self.model.new_location(pg_id).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
//                 })?);
//         }
//         let mut location = location.expect("assigned some value");
//         let mut buf = Vec::new();
//         loop {
//             match reader.read_event_into(&mut buf).map_err(|err| {
//                 ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
//             })? {
//                 Event::Empty(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                 })? {
//                     Self::TRANSITION => self.parse_transition(tag, reader, pg_id, location)?,
//                     tag_name => warn!("unknown empty tag {tag_name}, skipping"),
//                 },
//                 Event::Start(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                 })? {
//                     Self::ON_ENTRY | Self::ON_EXIT => {
//                         // 'on_entry' and 'on_exit' do the same thing:
//                         // extending the location with transitions applying the entry/exit procedures.
//                         // An 'on_entry' script is interpreted as a transition between the current location
//                         // and a new location created for the purpose.
//                         // Then, we proceed parsing from the new location.
//                         // Open questions:
//                         // - What if 'on_exit' is parsed before 'on_entry'?
//                         location = self.parse_on_entry_exit(tag, reader, pg_id, location)?
//                     }
//                     // Self::SCRIPT => self.parse_script(tag, reader, pg_id)?,
//                     // Unknown tag: skip till maching end tag
//                     tag_name => {
//                         warn!("unknown tag {tag_name}, skipping");
//                         reader
//                             .read_to_end_into(tag.to_end().into_owned().name(), &mut buf)
//                             .map_err(|err| {
//                                 ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
//                             })?;
//                     }
//                 },
//                 Event::End(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                 })? {
//                     Self::STATE => return Ok(()),
//                     name => {
//                         error!("unexpected end tag {name}");
//                         return Err(ParserError(
//                             reader.buffer_position(),
//                             ParserErrorType::UnexpectedEndTag(name.to_string()),
//                         ));
//                     }
//                 },
//                 Event::Text(_) | Event::Comment(_) => continue,
//                 // exits the loop when reaching end of file
//                 Event::Eof => todo!(),
//                 event => {
//                     return Err(ParserError(
//                         reader.buffer_position(),
//                         ParserErrorType::UnknownEvent(event.into_owned()),
//                     ))
//                 }
//             }
//             // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
//             buf.clear();
//         }
//     }

//     fn parse_transition<R: BufRead>(
//         &mut self,
//         tag: events::BytesStart,
//         reader: &mut Reader<R>,
//         pg_id: PgId,
//         state_id: CsLocation,
//     ) -> Result<(), ParserError> {
//         let mut event = None;
//         let mut target = None;
//         for attr in tag.attributes() {
//             let attr = attr
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Attr(err)))?;
//             match str::from_utf8(attr.key.as_ref())
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Utf8(err)))?
//             {
//                 Self::EVENT => {
//                     event = Some(String::from_utf8(attr.value.to_vec()).map_err(|err| {
//                         ParserError(
//                             reader.buffer_position(),
//                             ParserErrorType::Utf8(err.utf8_error()),
//                         )
//                     })?);
//                 }
//                 Self::TARGET => {
//                     target = Some(String::from_utf8(attr.value.to_vec()).map_err(|err| {
//                         ParserError(
//                             reader.buffer_position(),
//                             ParserErrorType::Utf8(err.utf8_error()),
//                         )
//                     })?);
//                 }
//                 name => warn!("unknown attribute {name}, ignoring"),
//             }
//         }

//         // If event is unspecified, the default is the NULL event
//         let event = event.unwrap_or(Self::NULL.to_string());
//         let action = match self.events.get(&event) {
//             Some(action) => *action,
//             None => {
//                 info!("new event {event}");
//                 self.model.new_action(pg_id).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
//                 })?
//             }
//         };
//         // make sure event is associated to action
//         let a = self.events.entry(event.clone()).or_insert(action);
//         assert_eq!(*a, action);
//         // check event has an associated activation variable
//         let raised = if let Some(raised) = self.vars.get(&event) {
//             *raised
//         } else {
//             // By default the variable is instantiated as false
//             self.model
//                 .new_var(pg_id, VarType::Boolean)
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Cs(err)))?
//         };

//         if let Some(target) = target {
//             let post = match self.states.get(&target) {
//                 Some(post) => *post,
//                 None => self.model.new_location(pg_id).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
//                 })?,
//             };
//             // make sure target is associated to post
//             let p = self.states.entry(target).or_insert(post);
//             assert_eq!(*p, post);

//             // finally add transition
//             let guard = CsFormula::new(pg_id, raised)
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Cs(err)))?;
//             self.model
//                 .add_effect(
//                     pg_id,
//                     action,
//                     raised,
//                     CsExpr::from_formula(CsFormula::new_false(pg_id)),
//                 )
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Cs(err)))?;
//             self.model
//                 .add_transition(pg_id, state_id, action, post, guard)
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Cs(err)))
//         } else {
//             warn!("transition with no target state, ignored");
//             Ok(())
//         }
//     }

//     fn parse_on_entry_exit<R: BufRead>(
//         &mut self,
//         tag: events::BytesStart<'_>,
//         reader: &mut Reader<R>,
//         pg_id: PgId,
//         state_cs_id: CsLocation,
//     ) -> Result<CsLocation, ParserError> {
//         let mut buf = Vec::new();
//         let mut post = state_cs_id;

//         if tag.attributes().last().is_some() {
//             error!("tag 'onentry' does not support any attribute, ignoring");
//         }

//         loop {
//             match reader.read_event_into(&mut buf).map_err(|err| {
//                 ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
//             })? {
//                 Event::Empty(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                 })? {
//                     Self::ASSIGN => {
//                         // Parsing 'assign' can create a new state
//                         post = self.parse_assign(reader, &tag, pg_id, post)?;
//                     }
//                     Self::RAISE => {
//                         post = self.parse_raise(reader, &tag, pg_id, post)?;
//                     }
//                     tag_name => error!("unknown empty tag {tag_name}, skipping"),
//                 },
//                 Event::Start(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                 })? {
//                     // Unknown tag: skip till maching end tag
//                     tag_name => {
//                         error!("unknown tag {tag_name}, skipping");
//                         reader
//                             .read_to_end_into(tag.to_end().into_owned().name(), &mut buf)
//                             .map_err(|err| {
//                                 ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
//                             })?;
//                     }
//                 },
//                 Event::End(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                 })? {
//                     Self::ON_ENTRY => return Ok(post),
//                     name => {
//                         error!("unexpected end tag {name}");
//                         return Err(ParserError(
//                             reader.buffer_position(),
//                             ParserErrorType::UnexpectedEndTag(name.to_string()),
//                         ));
//                     }
//                 },
//                 Event::Eof => todo!(),
//                 Event::Text(_) => warn!("skipping text"),
//                 Event::Comment(_) => warn!("skipping comment"),
//                 Event::CData(_) => todo!(),
//                 Event::Decl(_) => todo!(),
//                 Event::PI(_) => todo!(),
//                 Event::DocType(_) => todo!(),
//             }
//             // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
//             buf.clear();
//         }
//     }

//     fn parse_assign<R: BufRead>(
//         &mut self,
//         reader: &mut Reader<R>,
//         tag: &events::BytesStart<'_>,
//         pg_id: PgId,
//         pre: CsLocation,
//     ) -> Result<CsLocation, ParserError> {
//         // This is a 'location' in the sense of scxml, i.e., a variable
//         let mut location = None;
//         let mut expr = None;
//         for attr in tag.attributes() {
//             let attr = attr
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Attr(err)))?;
//             match str::from_utf8(attr.key.as_ref())
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Utf8(err)))?
//             {
//                 Self::LOCATION => {
//                     location = Some(String::from_utf8(attr.value.to_vec()).map_err(|err| {
//                         ParserError(
//                             reader.buffer_position(),
//                             ParserErrorType::Utf8(err.utf8_error()),
//                         )
//                     })?);
//                 }
//                 Self::EXPR => {
//                     expr = Some(String::from_utf8(attr.value.to_vec()).map_err(|err| {
//                         ParserError(
//                             reader.buffer_position(),
//                             ParserErrorType::Utf8(err.utf8_error()),
//                         )
//                     })?);
//                 }
//                 name => error!("unknown attribute {name}, ignoring"),
//             }
//         }
//         let location = location.ok_or(ParserError(
//             reader.buffer_position(),
//             ParserErrorType::MissingLocation,
//         ))?;
//         let var_id = self.vars.get(&location).ok_or(ParserError(
//             reader.buffer_position(),
//             ParserErrorType::UnknownVar(location),
//         ))?;
//         let expr = expr.ok_or(ParserError(
//             reader.buffer_position(),
//             ParserErrorType::MissingExpr,
//         ))?;
//         let effect: CsExpr = self.parse_expr(pg_id, expr)?;
//         // To assign the expression to the variable,
//         // we create a new 'assign' action
//         // and a new 'post' channel system location,
//         // then we add a transition that perform the assignment.
//         let assign = self
//             .model
//             .new_action(pg_id)
//             .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Cs(err)))?;
//         self.model
//             .add_effect(pg_id, assign, *var_id, effect)
//             .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Cs(err)))?;
//         let post = self
//             .model
//             .new_location(pg_id)
//             .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Cs(err)))?;
//         self.model
//             .add_transition(pg_id, pre, assign, post, CsFormula::new_true(pg_id))
//             .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Cs(err)))?;
//         Ok(post)
//     }

//     fn parse_expr(&self, pg_id: PgId, expr: String) -> Result<CsExpr, ParserError> {
//         // todo!()
//         Ok(CsExpr::from_formula(CsFormula::new_true(pg_id)))
//     }

//     fn parse_raise<R: BufRead>(
//         &mut self,
//         reader: &mut Reader<R>,
//         tag: &events::BytesStart<'_>,
//         pg_id: PgId,
//         post: CsLocation,
//     ) -> Result<CsLocation, ParserError> {
//         let mut post = post;
//         for attr in tag.attributes() {
//             let attr = attr
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Attr(err)))?;
//             match str::from_utf8(attr.key.as_ref())
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Utf8(err)))?
//             {
//                 Self::EVENT => {
//                     // To raise an event, we create a new Boolean variable associated to the name of the event
//                     // (unless such a variable exists already),
//                     // and an (anonymous) action triggering a transition to a next state
//                     // that sets the variable to true.
//                     // The raised event will then be interpreted as a transition
//                     // that has the associated variable as guard,
//                     // and setting the variable to false as an effect.
//                     let event = str::from_utf8(attr.value.as_ref()).map_err(|err| {
//                         ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                     })?;
//                     let raised = if let Some(raised) = self.vars.get(event) {
//                         *raised
//                     } else {
//                         self.model.new_var(pg_id, VarType::Boolean).map_err(|err| {
//                             ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
//                         })?
//                     };
//                     // Either 'event' was associated to no variable
//                     // or it was associated to 'raised' already.
//                     let _ = self.vars.insert(event.to_string(), raised);
//                     let raise = self.model.new_action(pg_id).map_err(|err| {
//                         ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
//                     })?;
//                     let after_raise = self.model.new_location(pg_id).map_err(|err| {
//                         ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
//                     })?;
//                     self.model
//                         .add_effect(
//                             pg_id,
//                             raise,
//                             raised,
//                             CsExpr::from_formula(CsFormula::new_true(pg_id)),
//                         )
//                         .map_err(|err| {
//                             ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
//                         })?;
//                     self.model
//                         .add_transition(pg_id, post, raise, after_raise, CsFormula::new_true(pg_id))
//                         .map_err(|err| {
//                             ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
//                         })?;
//                     post = after_raise;
//                 }
//                 name => error!("unknown attribute {name}, ignoring"),
//             }
//         }
//         Ok(post)
//     }
// }
