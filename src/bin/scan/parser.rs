use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::io::BufRead;
use std::str;
use std::str::Utf8Error;

use log::{debug, error, info, trace, warn};
use quick_xml::events::attributes::{AttrError, Attribute};
use quick_xml::{events, Error as XmlError};
use quick_xml::{events::Event, Reader};

use scan::{ChannelSystem, ChannelSystemBuilder, CsAction, CsFormula, CsLocation, PgId};

#[derive(Debug)]
pub(crate) enum ParserErrorType {
    Reader(XmlError),
    UnknownEvent(Event<'static>),
    Attr(AttrError),
    UnknownKey(String),
    Utf8(Utf8Error),
    Cs(scan::CsError),
    UnexpectedEndTag(String),
}

impl fmt::Display for ParserErrorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParserErrorType::UnknownEvent(_) => write!(f, "self:#?"),
            ParserErrorType::Attr(_) => write!(f, "self:#?"),
            ParserErrorType::Reader(err) => err.fmt(f),
            ParserErrorType::Utf8(err) => err.fmt(f),
            ParserErrorType::UnknownKey(_) => write!(f, "self:#?"),
            ParserErrorType::Cs(err) => err.fmt(f),
            ParserErrorType::UnexpectedEndTag(_) => write!(f, "self:#?"),
        }
    }
}

impl Error for ParserErrorType {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ParserErrorType::Reader(err) => Some(err),
            ParserErrorType::Utf8(err) => Some(err),
            ParserErrorType::Cs(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ParserError(usize, ParserErrorType);

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let byte = self.0;
        let err = &self.1;
        // Currently quick_xml only supports Reader byte position.
        // See https://github.com/tafia/quick-xml/issues/109
        write!(f, "parser error at byte {byte}: {err}")
    }
}

impl Error for ParserError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.1.source()
    }
}

#[derive(Debug)]
pub struct Parser {
    model: ChannelSystemBuilder,
    program_graphs: HashMap<String, PgId>,
    states: HashMap<String, CsLocation>,
    events: HashMap<String, CsAction>,
}

impl Parser {
    const STATE: &'static str = "state";
    const SCXML: &'static str = "scxml";
    const INITIAL: &'static str = "initial";
    const ID: &'static str = "id";
    const VERSION: &'static str = "version";
    const NAME: &'static str = "name";
    const XMLNS: &'static str = "xmlns";
    const DATAMODEL: &'static str = "datamodel";
    const BINDING: &'static str = "binding";
    const TRANSITION: &'static str = "transition";
    const TARGET: &'static str = "target";
    const EVENT: &'static str = "event";
    const NULL: &'static str = "NULL";

    pub fn parse<R: BufRead>(reader: &mut Reader<R>) -> Result<ChannelSystem, ParserError> {
        let mut parser = Self {
            model: ChannelSystemBuilder::default(),
            program_graphs: HashMap::default(),
            states: HashMap::default(),
            events: HashMap::default(),
        };
        let mut buf = Vec::new();
        info!("begin parsing");
        loop {
            info!("processing new event");
            match reader.read_event_into(&mut buf).map_err(|err| {
                ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
            })? {
                Event::Start(tag) => {
                    match str::from_utf8(tag.name().as_ref()).map_err(|err| {
                        ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                    })? {
                        Self::SCXML => {
                            info!("found new scxml open tag");
                            parser.parse_scxml(tag, reader)?;
                        }
                        // Unknown tag: skip till maching end tag
                        tag_name => {
                            warn!("found unknown tag {tag_name}, skipping");
                            reader
                                .read_to_end_into(tag.to_end().into_owned().name(), &mut buf)
                                .map_err(|err| {
                                    ParserError(
                                        reader.buffer_position(),
                                        ParserErrorType::Reader(err),
                                    )
                                })?;
                        }
                    }
                }
                // exits the loop when reaching end of file
                Event::Eof => {
                    info!("parsing completed");
                    let model = parser.model.build();
                    return Ok(model);
                }
                Event::End(tag) => {
                    let name = str::from_utf8(tag.name().as_ref())
                        .map_err(|err| {
                            ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                        })?
                        .to_string();
                    error!("unexpected end tag {name}");
                    return Err(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnexpectedEndTag(name),
                    ));
                }
                Event::Empty(_) => todo!(),
                Event::Text(_) => warn!("skipping text"),
                Event::Comment(_) => warn!("skipping comment"),
                Event::CData(_) => todo!(),
                Event::Decl(_) => todo!(),
                Event::PI(_) => todo!(),
                Event::DocType(_) => todo!(),
            }
            // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
            buf.clear();
        }
    }

    fn parse_scxml<R: BufRead>(
        &mut self,
        tag: events::BytesStart,
        reader: &mut Reader<R>,
    ) -> Result<(), ParserError> {
        // let mut initial = None;
        // let mut name = None;
        let mut xmlns = None;
        let mut version = None;
        let mut datamodel = None;
        let mut binding = None;
        let pg_id = self.model.new_program_graph();
        for attr in tag
            .attributes()
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()
            .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Attr(err)))?
        {
            // match attr.key.as_ref() {
            match str::from_utf8(attr.key.as_ref())
                .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Utf8(err)))?
            {
                Self::INITIAL => {
                    // initial = Some(attr.value.as_ref());
                    let cs_id = self.model.initial_location(pg_id).expect("pg_id exists");
                    let id = str::from_utf8(attr.value.as_ref()).map_err(|err| {
                        ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                    })?;
                    self.states.insert(id.to_owned(), cs_id);
                }
                Self::VERSION => version = Some(attr.value.as_ref()),
                Self::XMLNS => xmlns = Some(attr.value.as_ref()),
                Self::DATAMODEL => datamodel = Some(attr.value.as_ref()),
                Self::BINDING => binding = Some(attr.value.as_ref()),
                Self::NAME => {
                    let name = str::from_utf8(attr.value.as_ref()).map_err(|err| {
                        ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                    })?;
                    self.program_graphs.insert(name.to_owned(), pg_id);
                }
                key => {
                    return Err(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    ));
                }
            }
        }
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf).map_err(|err| {
                ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
            })? {
                Event::Empty(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
                    ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                })? {
                    _ => continue,
                },
                Event::Start(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
                    ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                })? {
                    Self::STATE => {
                        self.parse_state(tag, reader, pg_id)?;
                    }
                    // Self::DATAMODEL => todo!(),
                    // Unknown tag: skip till maching end tag
                    tag_name => {
                        warn!("unknown tag {tag_name}, skipping");
                        reader
                            .read_to_end_into(tag.to_end().into_owned().name(), &mut buf)
                            .map_err(|err| {
                                ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
                            })?;
                    }
                },
                Event::End(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
                    ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                })? {
                    // Self::STATE => return Ok(()),
                    Self::SCXML => {
                        info!("done parsing scxml");
                        return Ok(());
                    }
                    name => {
                        error!("unexpected end tag {name}");
                        return Err(ParserError(
                            reader.buffer_position(),
                            ParserErrorType::UnexpectedEndTag(name.to_string()),
                        ));
                    }
                },
                // exits the loop when reaching end of file
                Event::Eof => todo!(),
                Event::Text(_) => warn!("skipping text"),
                Event::Comment(_) => warn!("skipping comment"),
                Event::CData(_) => todo!(),
                Event::Decl(_) => todo!(),
                Event::PI(_) => todo!(),
                Event::DocType(_) => todo!(),
            }
            // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
            buf.clear();
        }
    }

    fn parse_state<R: BufRead>(
        &mut self,
        tag: events::BytesStart,
        reader: &mut Reader<R>,
        pg_id: PgId,
    ) -> Result<(), ParserError> {
        let mut state_cs_id = None;
        for attr in tag.attributes() {
            let attr = attr
                .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Attr(err)))?;
            match str::from_utf8(attr.key.as_ref())
                .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Utf8(err)))?
            {
                Self::ID => {
                    let id = str::from_utf8(attr.value.as_ref()).map_err(|err| {
                        ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                    })?;
                    if let Some(state) = self.states.get(id) {
                        state_cs_id = Some(*state);
                    } else {
                        let state = self.model.new_location(pg_id).map_err(|err| {
                            ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
                        })?;
                        state_cs_id = Some(state);
                        let previous = self.states.insert(id.to_owned(), state);
                        assert!(previous.is_none(), "states did not contain the key");
                    }
                }
                name => warn!("unknown attribute {name}, ignoring"),
            }
        }
        if state_cs_id.is_none() {
            state_cs_id =
                Some(self.model.new_location(pg_id).map_err(|err| {
                    ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
                })?);
        }
        let state_cs_id = state_cs_id.expect("assigned some value");
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf).map_err(|err| {
                ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
            })? {
                Event::Empty(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
                    ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                })? {
                    Self::TRANSITION => self.parse_transition(tag, reader, pg_id, state_cs_id)?,
                    tag_name => warn!("unknown empty tag {tag_name}, skipping"),
                },
                Event::Start(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
                    ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                })? {
                    // Unknown tag: skip till maching end tag
                    tag_name => {
                        warn!("unknown tag {tag_name}, skipping");
                        reader
                            .read_to_end_into(tag.to_end().into_owned().name(), &mut buf)
                            .map_err(|err| {
                                ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
                            })?;
                    }
                },
                Event::End(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
                    ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                })? {
                    Self::STATE => return Ok(()),
                    name => {
                        error!("unexpected end tag {name}");
                        return Err(ParserError(
                            reader.buffer_position(),
                            ParserErrorType::UnexpectedEndTag(name.to_string()),
                        ));
                    }
                },
                Event::Text(_) | Event::Comment(_) => continue,
                // exits the loop when reaching end of file
                Event::Eof => todo!(),
                event => {
                    return Err(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownEvent(event.into_owned()),
                    ))
                }
            }
            // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
            buf.clear();
        }
    }

    fn parse_transition<R: BufRead>(
        &mut self,
        tag: events::BytesStart,
        reader: &mut Reader<R>,
        pg_id: PgId,
        state_id: CsLocation,
    ) -> Result<(), ParserError> {
        let mut event = None;
        let mut target = None;
        for attr in tag.attributes() {
            let attr = attr
                .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Attr(err)))?;
            match str::from_utf8(attr.key.as_ref())
                .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Utf8(err)))?
            {
                Self::EVENT => {
                    event = Some(String::from_utf8(attr.value.to_vec()).map_err(|err| {
                        ParserError(
                            reader.buffer_position(),
                            ParserErrorType::Utf8(err.utf8_error()),
                        )
                    })?);
                }
                Self::TARGET => {
                    target = Some(String::from_utf8(attr.value.to_vec()).map_err(|err| {
                        ParserError(
                            reader.buffer_position(),
                            ParserErrorType::Utf8(err.utf8_error()),
                        )
                    })?);
                }
                name => warn!("unknown attribute {name}, ignoring"),
            }
        }

        // If event is unspecified, the default is the NULL event
        let event = event.unwrap_or(Self::NULL.to_string());
        let action = match self.events.get(&event) {
            Some(action) => *action,
            None => {
                info!("new event {event}");
                self.model.new_action(pg_id).map_err(|err| {
                    ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
                })?
            }
        };
        // make sure event is associated to action
        let a = self.events.entry(event).or_insert(action);
        assert_eq!(*a, action);

        if let Some(target) = target {
            let post = match self.states.get(&target) {
                Some(post) => *post,
                None => self.model.new_location(pg_id).map_err(|err| {
                    ParserError(reader.buffer_position(), ParserErrorType::Cs(err))
                })?,
            };
            // make sure target is associated to post
            let p = self.states.entry(target).or_insert(post);
            assert_eq!(*p, post);

            // finally add transition
            let true_formula = CsFormula::new_true(pg_id);
            self.model
                .add_transition(pg_id, state_id, action, post, true_formula)
                .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Cs(err)))
        } else {
            warn!("transition with no target state, ignored");
            Ok(())
        }
    }
}
