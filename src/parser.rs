mod bt;
mod fsm;
pub(crate) mod vocabulary;

use std::collections::HashMap;
use std::io::BufRead;
use std::path::PathBuf;
use std::str;
use std::str::Utf8Error;

use anyhow::anyhow;
use log::{error, info, trace, warn};
use quick_xml::events::attributes::{AttrError, Attribute};
use quick_xml::{events, Error as XmlError};
use quick_xml::{events::Event, Reader};
use thiserror::Error;

pub use self::bt::*;
pub use self::fsm::*;
use self::vocabulary::*;
use super::model::{ChannelSystem, ChannelSystemBuilder, CsError};

#[derive(Error, Debug)]
pub enum ParserErrorType {
    #[error("reader failed")]
    Reader(#[from] XmlError),
    #[error("an unknown or unexpected event was received: `{0:?}`")]
    UnknownEvent(Event<'static>),
    #[error("error from an attribute")]
    Attr(#[from] AttrError),
    #[error("unknown key: `{0}`")]
    UnknownKey(String),
    #[error("unknown key val: `{0}`")]
    UnknownVal(String),
    #[error("utf8 error")]
    Utf8(#[from] Utf8Error),
    #[error("channel system error")]
    Cs(#[from] CsError),
    #[error("unexpected start tag: `{0}`")]
    UnexpectedStartTag(String),
    #[error("unexpected end tag: `{0}`")]
    UnexpectedEndTag(String),
    #[error("location does not exist")]
    MissingLocation,
    #[error("unknown variable `{0}`")]
    UnknownVar(String),
    #[error("missing `expr` attribute")]
    MissingExpr,
    #[error("missing attribute `{0}`")]
    MissingAttr(String),
    #[error("open tags have not been closed")]
    UnclosedTags,
    #[error("`{0}` has already been declared")]
    AlreadyDeclared(String),
    #[error("unknown model of computation: `{0}`")]
    UnknownMoC(String),
    #[error("unknown type of skill: `{0}`")]
    UnknownSkillType(String),
    #[error("not in a state")]
    NotAState,
    #[error("behavior tree missing root node")]
    MissingBtRootNode,
    #[error("something went wrong parsing EcmaScript code")]
    EcmaScriptParsing,
}

#[derive(Error, Debug)]
#[error("parser error at byte `{0}`")]
pub struct ParserError(pub(crate) usize, #[source] pub(crate) ParserErrorType);

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConvinceTag {
    Specification,
    Model,
    Properties,
    ComponentList,
    SkillList,
}

impl From<ConvinceTag> for &'static str {
    fn from(value: ConvinceTag) -> Self {
        match value {
            ConvinceTag::Specification => TAG_SPECIFICATION,
            ConvinceTag::Model => TAG_MODEL,
            ConvinceTag::Properties => TAG_PROPERTIES,
            ConvinceTag::ComponentList => TAG_COMPONENT_LIST,
            ConvinceTag::SkillList => TAG_SKILL_LIST,
        }
    }
}

#[derive(Debug)]
pub struct Parser {
    pub(crate) skill_list: HashMap<String, Skill>,
    pub(crate) component_list: HashMap<String, Component>,
    // interfaces: PathBuf,
    // types: PathBuf,
    // properties: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub enum SkillType {
    Action,
    Condition,
}

impl TryFrom<String> for SkillType {
    type Error = ParserErrorType;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            OPT_ACTION => Ok(SkillType::Action),
            OPT_CONDITION => Ok(SkillType::Condition),
            _ => Err(ParserErrorType::UnknownSkillType(value)),
        }
    }
}

#[derive(Debug)]
pub enum MoC {
    Fsm(Fsm),
    Bt(Bt),
}

#[derive(Debug)]
pub struct Skill {
    pub(crate) skill_type: Option<SkillType>,
    pub(crate) moc: MoC,
}

#[derive(Debug)]
pub struct Component {
    pub(crate) moc: MoC,
}

impl Parser {
    pub fn parse<R: BufRead>(reader: &mut Reader<R>) -> anyhow::Result<Parser> {
        let mut spec = Parser {
            skill_list: HashMap::new(),
            component_list: HashMap::new(),
        };
        let mut buf = Vec::new();
        let mut stack = Vec::new();
        info!("begin parsing");
        loop {
            match reader.read_event_into(&mut buf)? {
                Event::Start(tag) => {
                    let tag_name = tag.name();
                    let tag_name = str::from_utf8(tag_name.as_ref())?;
                    trace!("'{tag_name}' open tag");
                    match tag_name {
                        TAG_SPECIFICATION if stack.is_empty() => {
                            stack.push(ConvinceTag::Specification);
                        }
                        TAG_MODEL
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::Specification) =>
                        {
                            stack.push(ConvinceTag::Model);
                        }
                        TAG_SKILL_LIST
                            if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
                        {
                            stack.push(ConvinceTag::SkillList);
                        }
                        TAG_PROPERTIES
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::Specification) =>
                        {
                            stack.push(ConvinceTag::Properties);
                        }
                        TAG_COMPONENT_LIST
                            if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
                        {
                            stack.push(ConvinceTag::ComponentList);
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
                        TAG_SKILL
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::SkillList) =>
                        {
                            spec.parse_skill(tag, reader)?;
                        }
                        TAG_COMPONENT_DECLARATION
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::ComponentList) =>
                        {
                            spec.parse_comp_declaration(tag, reader)?;
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
                    // let model = parser.model.build();
                    // return Ok(model);
                    break;
                }
            }
            // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
            buf.clear();
        }
        Ok(spec)
    }

    pub fn build_model(self) -> ChannelSystem {
        let cs = ChannelSystemBuilder::new();
        cs.build()
    }

    fn parse_skill<R: BufRead>(
        &mut self,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
    ) -> anyhow::Result<()> {
        let mut skill_id: Option<String> = None;
        let mut skill_type: Option<String> = None;
        let mut moc: Option<String> = None;
        let mut path: Option<String> = None;
        for attr in tag
            .attributes()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_ID => {
                    skill_id = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_TYPE => {
                    skill_type = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_MOC => {
                    moc = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_PATH => {
                    path = Some(String::from_utf8(attr.value.into_owned())?);
                }
                key => {
                    error!("found unknown attribute {key}");
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        let skill_id = skill_id.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_ID.to_string())
        )))?;
        let skill_type = skill_type.map(SkillType::try_from).transpose()?;
        let path = path.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_PATH.to_string())
        )))?;
        let path = PathBuf::from(path);
        let moc = moc.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_MOC.to_string())
        )))?;
        let moc = match moc.as_str() {
            "fsm" => {
                info!("creating reader from file {0}", path.display());
                let mut reader = Reader::from_file(path)?;
                let fsm = Fsm::parse_skill(&mut reader)?;
                MoC::Fsm(fsm)
            }
            "bt" => {
                info!("creating reader from file {0}", path.display());
                let mut reader = Reader::from_file(path)?;
                let bt = Bt::parse_skill(&mut reader)?.pop().unwrap();
                MoC::Bt(bt)
            }
            _ => todo!(),
        };
        let skill = Skill { skill_type, moc };
        // Here it should be checked that no skill was already in the list under the same name
        self.skill_list.insert(skill_id, skill);
        Ok(())
    }

    fn parse_comp_declaration<R: BufRead>(
        &mut self,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
    ) -> anyhow::Result<()> {
        let mut comp_id: Option<String> = None;
        let mut moc: Option<String> = None;
        let mut path: Option<String> = None;
        for attr in tag
            .attributes()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_ID => {
                    comp_id = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_MOC => {
                    moc = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_PATH => {
                    path = Some(String::from_utf8(attr.value.into_owned())?);
                }
                key => {
                    error!("found unknown attribute {key}");
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        let comp_id = comp_id.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_ID.to_string())
        )))?;
        let path = path.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_PATH.to_string())
        )))?;
        let path = PathBuf::from(path);
        let moc = moc.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_MOC.to_string())
        )))?;
        let path = PathBuf::from(path);
        let moc = match moc.as_str() {
            "fsm" => {
                info!("creating reader from file {0}", path.display());
                let mut reader = Reader::from_file(path)?;
                let fsm = Fsm::parse_skill(&mut reader)?;
                MoC::Fsm(fsm)
            }
            "bt" => {
                info!("creating reader from file {0}", path.display());
                let mut reader = Reader::from_file(path)?;
                let bt = Bt::parse_skill(&mut reader)?.pop().unwrap();
                MoC::Bt(bt)
            }
            _ => {
                return Err(anyhow!(ParserError(
                    reader.buffer_position(),
                    ParserErrorType::UnknownVal(moc.to_owned())
                )))
            }
        };
        let component = Component { moc };
        // Here it should be checked that no component was already in the list under the same name
        self.component_list.insert(comp_id.to_owned(), component);
        Ok(())
    }
}
