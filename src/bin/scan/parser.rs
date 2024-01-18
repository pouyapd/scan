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

use scan::{ChannelSystem, ChannelSystemBuilder, CsLocation, PgId};

#[derive(Debug)]
pub(crate) enum ParserErrorType {
    Reader(XmlError),
    UnknownEvent(Event<'static>),
    Attr(AttrError),
    UnknownKey(String),
    Utf8(Utf8Error),
}

impl fmt::Display for ParserErrorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParserErrorType::UnknownEvent(_) => write!(f, "self:#?"),
            ParserErrorType::Attr(_) => write!(f, "self:#?"),
            ParserErrorType::Reader(err) => err.fmt(f),
            ParserErrorType::Utf8(err) => err.fmt(f),
            ParserErrorType::UnknownKey(_) => write!(f, "self:#?"),
        }
    }
}

impl Error for ParserErrorType {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ParserErrorType::Reader(err) => Some(err),
            ParserErrorType::Utf8(err) => Some(err),
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

    pub fn parse<R: BufRead>(reader: &mut Reader<R>) -> Result<ChannelSystem, ParserError> {
        let mut parser = Self {
            model: ChannelSystemBuilder::default(),
            program_graphs: HashMap::default(),
            states: HashMap::default(),
        };
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf).map_err(|err| {
                ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
            })? {
                Event::Start(tag) => {
                    match str::from_utf8(tag.name().as_ref()).map_err(|err| {
                        ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                    })? {
                        Self::SCXML => parser.parse_scxml(tag, reader)?,
                        // Unknown tag: skip till maching end tag
                        _ => {
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
                Event::Text(_) | Event::Comment(_) => continue,
                // exits the loop when reaching end of file
                Event::Eof => {
                    let model = parser.model.build();
                    return Ok(model);
                }
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
        let mut states: HashMap<Vec<u8>, CsLocation> = HashMap::new();
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
                    let id = self.model.initial_location(pg_id).expect("pg_id exists");
                    states.insert(attr.value.into_owned(), id);
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
                    // Self::STATE => {
                    //     self.parse_state(tag, reader, pg_id, &mut states)?;
                    // }
                    // Self::DATAMODEL => todo!(),
                    // Unknown tag: skip till maching end tag
                    _ => {
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
                    Self::SCXML => return Ok(()),
                    _ => todo!(),
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

    fn parse_state<R: BufRead>(
        &mut self,
        tag: events::BytesStart,
        reader: &mut Reader<R>,
        pg_id: PgId,
        states: &mut HashMap<Vec<u8>, CsLocation>,
    ) -> Result<(), ParserError> {
        // for attr in tag.attributes() {
        //     let attr = attr.map_err(|err| {
        //         ParserError(reader.buffer_position(), ParserErrorType::Attr(err))
        //     })?;
        //     match attr.key.as_ref() {
        //         Self::ID => {
        //             if !states.contains_key(attr.value.as_ref()) {
        //                 // model.new_location(pg_id)
        //             }
        //         }
        //         key => {
        //             return Err(ParserError(
        //                 reader.buffer_position(),
        //                 ParserErrorType::UnknownAttr(key.to_owned()),
        //             ));
        //         }
        //     }
        // }
        Ok(())
    }
}
