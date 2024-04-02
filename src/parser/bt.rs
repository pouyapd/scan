use std::io::BufRead;
use std::str;

use anyhow::anyhow;
use log::{error, info, trace, warn};
use quick_xml::events::attributes::{AttrError, Attribute};
use quick_xml::{events, events::Event, Reader};

use super::vocabulary::*;
use crate::{ParserError, ParserErrorType};

#[derive(Debug, Clone)]
pub enum Bt {
    Root(Box<Bt>),
    RSeq(Vec<Box<Bt>>),
    RFbk(Vec<Box<Bt>>),
    MSeq(Vec<Box<Bt>>),
    MFbk(Vec<Box<Bt>>),
    Invr(Box<Bt>),
    LAct(String),
    LCnd(String),
}

impl Bt {
    pub(super) fn parse_skill<R: BufRead>(reader: &mut Reader<R>) -> anyhow::Result<Vec<Self>> {
        let mut bts = Vec::new();
        let mut buf = Vec::new();
        info!("parsing bt");
        loop {
            match reader.read_event_into(&mut buf)? {
                Event::Start(tag) => {
                    let tag_name = tag.name();
                    let tag_name = str::from_utf8(tag_name.as_ref())?;
                    trace!("'{tag_name}' open tag");
                    match tag_name {
                        TAG_ROOT | TAG_BEHAVIOR_TREE => {
                            return Self::parse_skill(reader);
                        }
                        TAG_REACTIVE_SEQUENCE => {
                            bts.push(Bt::RSeq(
                                Self::parse_skill(reader)?
                                    .into_iter()
                                    .map(Box::new)
                                    .collect(),
                            ));
                        }
                        TAG_REACTIVE_FALLBACK => {
                            bts.push(Bt::RFbk(
                                Self::parse_skill(reader)?
                                    .into_iter()
                                    .map(Box::new)
                                    .collect(),
                            ));
                        }
                        // Unknown tag: skip till maching end tag
                        _ => {
                            warn!("unknown or unexpected tag {tag_name}, skipping");
                            reader.read_to_end_into(tag.to_end().into_owned().name(), &mut buf)?;
                        }
                    }
                }
                Event::End(_tag) => {
                    return Ok(bts);
                }
                Event::Empty(tag) => {
                    let tag_name = tag.name();
                    let tag_name = str::from_utf8(tag_name.as_ref())?;
                    trace!("'{tag_name}' empty tag");
                    // let tag_name = ConvinceTag::from(tag_name.as_str());
                    match tag_name {
                        TAG_ACTION => {
                            bts.push(Self::parse_action(tag, reader)?);
                        }
                        TAG_CONDITION => {
                            bts.push(Self::parse_condition(tag, reader)?);
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
                Event::CData(_) => continue,
                Event::Decl(_) => continue, // parser.parse_xml_declaration(tag)?,
                Event::PI(_) => continue,
                Event::DocType(_) => continue,
                // exits the loop when reaching end of file
                Event::Eof => {
                    return Err(anyhow!(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnclosedTags,
                    )));
                }
            }
            // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
            buf.clear();
        }
    }

    fn parse_action<R: BufRead>(
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
    ) -> anyhow::Result<Bt> {
        let mut id: Option<String> = None;
        for attr in tag
            .attributes()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_BT_ID => {
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
        let action = Bt::LAct(id);
        Ok(action)
    }

    fn parse_condition<R: BufRead>(
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
    ) -> anyhow::Result<Bt> {
        let mut id: Option<String> = None;
        for attr in tag
            .attributes()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_BT_ID => {
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
        let condition = Bt::LCnd(id);
        Ok(condition)
    }
}
