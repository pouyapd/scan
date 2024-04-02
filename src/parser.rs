mod bt;
mod fsm;
mod vocabulary;

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
}

#[derive(Error, Debug)]
#[error("parser error at byte `{0}`")]
pub struct ParserError(pub(crate) usize, #[source] pub(crate) ParserErrorType);

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConvinceTag {
    Specification,
    Model,
    Properties,
    // Scxml,
    ComponentList,
    ComponentDeclaration { comp_id: String, interface: String },
    // ComponentDefinition,
    // BlackBoard,
    SkillList,
    // BtToSkillInterface,
    // Bt,
    SkillDeclaration { skill_id: String, interface: String },
    // SkillDefinition,
    // StructList,
    // Enumeration,
    // Service,
    // Struct,
    // StructData,
    // Enum,
    // Function,
}

impl From<ConvinceTag> for &'static str {
    fn from(value: ConvinceTag) -> Self {
        match value {
            ConvinceTag::Specification => TAG_SPECIFICATION,
            ConvinceTag::Model => TAG_MODEL,
            ConvinceTag::Properties => TAG_PROPERTIES,
            // ConvinceTag::Scxml => TAG_SCXML,
            ConvinceTag::ComponentList => TAG_COMPONENT_LIST,
            ConvinceTag::ComponentDeclaration { .. } => TAG_COMPONENT_DECLARATION,
            // ConvinceTag::ComponentDefinition => TAG_COMPONENT_DEFINITION,
            // ConvinceTag::BlackBoard => TAG_BLACKBOARD,
            ConvinceTag::SkillList => TAG_SKILL_LIST,
            // ConvinceTag::BtToSkillInterface => TAG_BTTOSKILLINTERFACE,
            // ConvinceTag::Bt => TAG_BT,
            ConvinceTag::SkillDeclaration { .. } => TAG_SKILL_DECLARATION,
            // ConvinceTag::SkillDefinition => TAG_SKILL_DEFINITION,
            // ConvinceTag::StructList => TAG_STRUCT_LIST,
            // ConvinceTag::Enumeration => TAG_ENUMERATION,
            // ConvinceTag::Service => TAG_SERVICE,
            // ConvinceTag::Struct => TAG_STRUCT,
            // ConvinceTag::StructData => TAG_STRUCT_DATA,
            // ConvinceTag::Enum => TAG_ENUM,
            // ConvinceTag::Function => TAG_FUNCTION,
        }
    }
}

#[derive(Debug)]
pub struct Parser {
    pub(crate) task_plan: Option<String>,
    pub(crate) skill_list: HashMap<String, SkillDeclaration>,
    pub(crate) component_list: HashMap<String, ComponentDeclaration>,
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
pub struct SkillDeclaration {
    pub(crate) interface: String,
    pub(crate) skill_type: SkillType,
    pub(crate) moc: MoC,
}

#[derive(Debug)]
pub struct ComponentDeclaration {
    pub(crate) interface: String,
    pub(crate) moc: MoC,
}

impl Parser {
    pub fn parse<R: BufRead>(reader: &mut Reader<R>) -> anyhow::Result<Parser> {
        let mut spec = Parser {
            task_plan: None,
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
                            spec.parse_skill_list(tag, reader)?;
                            stack.push(ConvinceTag::SkillList);
                        }
                        TAG_SKILL_DECLARATION
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::SkillList) =>
                        {
                            let tag = spec.parse_skill_declaration(tag, reader)?;
                            stack.push(tag);
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
                        TAG_COMPONENT_DECLARATION
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::ComponentList) =>
                        {
                            let tag = spec.parse_comp_declaration(tag, reader)?;
                            stack.push(tag);
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
                        TAG_SKILL_DEFINITION
                            if matches!(
                                stack.last(),
                                Some(ConvinceTag::SkillDeclaration { .. })
                            ) =>
                        {
                            if let Some(ConvinceTag::SkillDeclaration {
                                skill_id,
                                interface,
                            }) = stack.last()
                            {
                                spec.parse_skill_definition(
                                    skill_id.to_owned(),
                                    interface.to_owned(),
                                    tag,
                                    reader,
                                )?;
                            } else {
                                panic!("match guard prevents this");
                            }
                        }
                        TAG_COMPONENT_DEFINITION
                            if matches!(
                                stack.last(),
                                Some(ConvinceTag::ComponentDeclaration { .. })
                            ) =>
                        {
                            if let Some(ConvinceTag::ComponentDeclaration { comp_id, interface }) =
                                stack.last()
                            {
                                spec.parse_comp_definition(
                                    comp_id.to_owned(),
                                    interface.to_owned(),
                                    tag,
                                    reader,
                                )?;
                            } else {
                                panic!("match guard prevents this");
                            }
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

    fn parse_skill_list<R: BufRead>(
        &mut self,
        tag: events::BytesStart<'_>,
        reader: &Reader<R>,
    ) -> anyhow::Result<()> {
        for attr in tag
            .attributes()
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_TASK_PLAN => {
                    let skill_id = str::from_utf8(attr.value.as_ref())?;
                    self.task_plan = Some(skill_id.to_string());
                }
                key => {
                    error!("found unknown attribute {key} in {ATTR_TASK_PLAN}",);
                    return Err(anyhow!(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        Ok(())
    }

    fn parse_skill_declaration<R: BufRead>(
        &mut self,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
    ) -> anyhow::Result<ConvinceTag> {
        let mut skill_id: Option<String> = None;
        let mut interface: Option<String> = None;
        for attr in tag
            .attributes()
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_ID => {
                    skill_id = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_INTERFACE => {
                    interface = Some(String::from_utf8(attr.value.into_owned())?);
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
        let interface = interface.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_INTERFACE.to_string())
        )))?;
        Ok(ConvinceTag::SkillDeclaration {
            skill_id,
            interface,
        })
    }

    fn parse_comp_declaration<R: BufRead>(
        &mut self,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
    ) -> anyhow::Result<ConvinceTag> {
        let mut comp_id: Option<String> = None;
        let mut interface: Option<String> = None;
        for attr in tag
            .attributes()
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_ID => {
                    comp_id = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_INTERFACE => {
                    interface = Some(String::from_utf8(attr.value.into_owned())?);
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
        let interface = interface.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_INTERFACE.to_string())
        )))?;
        Ok(ConvinceTag::ComponentDeclaration { comp_id, interface })
    }

    fn parse_skill_definition<R: BufRead>(
        &mut self,
        skill_id: String,
        interface: String,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
    ) -> anyhow::Result<()> {
        let mut type_skill: Option<String> = None;
        let mut moc: Option<String> = None;
        let mut path: Option<String> = None;
        for attr in tag
            .attributes()
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_TYPE => {
                    type_skill = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_MOC => {
                    moc = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_PATH => {
                    path = Some(String::from_utf8(attr.value.into_owned())?);
                }
                key => {
                    error!("found unknown attribute {key} in {TAG_SKILL_DEFINITION}");
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        let skill_type = type_skill.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_TYPE.to_string())
        )))?;
        let skill_type = SkillType::try_from(skill_type)?;
        let moc = moc.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_MOC.to_string())
        )))?;
        // let moc = MoC::try_from(moc)?;
        let path = path.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_PATH.to_string())
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
                // let xml = read_to_string(path)?;
                // let mut reader = Reader::from_str(&xml);
                // let bt = bt_semantics::parse_xml(&mut reader).pop().unwrap();
                MoC::Bt(bt)
            }
            _ => todo!(),
        };
        let skill = SkillDeclaration {
            skill_type,
            interface,
            moc,
        };
        // Here it should be checked that no skill was already in the list under the same name
        self.skill_list.insert(skill_id, skill);
        Ok(())
    }

    fn parse_comp_definition<R: BufRead>(
        &mut self,
        comp_id: String,
        interface: String,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
    ) -> anyhow::Result<()> {
        let mut moc: Option<String> = None;
        let mut path: Option<String> = None;
        for attr in tag
            .attributes()
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ATTR_MOC => {
                    moc = Some(String::from_utf8(attr.value.into_owned())?);
                }
                ATTR_PATH => {
                    path = Some(String::from_utf8(attr.value.into_owned())?);
                }
                key => {
                    error!("found unknown attribute {key} in {TAG_COMPONENT_DEFINITION}");
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        let moc = moc.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_MOC.to_string())
        )))?;
        let path = path.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ATTR_PATH.to_string())
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
                // let xml = read_to_string(path)?;
                // let mut reader = Reader::from_str(&xml);
                // let bt = bt_semantics::parse_xml(&mut reader).pop().unwrap();
                MoC::Bt(bt)
            }
            _ => todo!(),
        };
        let component = ComponentDeclaration { interface, moc };
        // Here it should be checked that no component was already in the list under the same name
        self.component_list.insert(comp_id.to_owned(), component);
        Ok(())
    }
}

// #[derive(Debug)]
// pub struct Parser {
//     model: ChannelSystemBuilder,
//     skills: HashMap<String, PgId>,
//     states: HashMap<String, CsLocation>,
//     events: HashMap<String, CsAction>,
//     vars: HashMap<String, CsVar>,
// }

// impl Parser {
//     const SPECIFICATION: &'static str = "specification";
//     const MODEL: &'static str = "model";
//     const PROPERTIES: &'static str = "properties";
//     const COMPONENT_LIST: &'static str = "componentList";
//     const BLACK_BOARD: &'static str = "blackBoard";
//     const SKILL_LIST: &'static str = "skillList";
//     const BT_TO_SKILL_INTERFACE: &'static str = "btToSkillInterface";
//     const BT: &'static str = "bt";
//     const COMPONENT: &'static str = "component";
//     const SKILL_DECLARATION: &'static str = "skillDeclaration";
//     const SKILL_DEFINITION: &'static str = "skillDefinition";
//     const FILE: &'static str = "file";
//     const STATE: &'static str = "state";
//     const SCXML: &'static str = "scxml";
//     const INITIAL: &'static str = "initial";
//     const ID: &'static str = "id";
//     const MOC: &'static str = "moc";
//     const PATH: &'static str = "path";
//     const FSM: &'static str = "fsm";
//     const INTERFACE: &'static str = "interface";
//     const VERSION: &'static str = "version";
//     const NAME: &'static str = "name";
//     const XMLNS: &'static str = "xmlns";
//     const DATAMODEL: &'static str = "datamodel";
//     const DATA: &'static str = "data";
//     const TYPE: &'static str = "type";
//     const BOOL: &'static str = "bool";
//     const INT: &'static str = "int";
//     const UNIT: &'static str = "unit";
//     const BINDING: &'static str = "binding";
//     const TRANSITION: &'static str = "transition";
//     const TARGET: &'static str = "target";
//     const EVENT: &'static str = "event";
//     const ON_ENTRY: &'static str = "onentry";
//     const ON_EXIT: &'static str = "onexit";
//     const NULL: &'static str = "NULL";
//     const SCRIPT: &'static str = "script";
//     const ASSIGN: &'static str = "assign";
//     const LOCATION: &'static str = "location";
//     const EXPR: &'static str = "expr";
//     const RAISE: &'static str = "raise";
//     const STRUCT_LIST: &'static str = "structList";
//     const ENUMERATION: &'static str = "enumeration";
//     const SERVICE: &'static str = "service";
//     const STRUCT: &'static str = "struct";
//     const STRUCT_DATA: &'static str = "structData";
//     const FIELD_ID: &'static str = "fieldId";
//     const ENUM: &'static str = "enum";
//     const FUNCTION: &'static str = "function";

//     pub fn parse<R: BufRead>(reader: &mut Reader<R>) -> anyhow::Result<ChannelSystem> {
//         let mut parser = Self {
//             model: ChannelSystemBuilder::default(),
//             skills: HashMap::default(),
//             states: HashMap::default(),
//             events: HashMap::default(),
//             vars: HashMap::default(),
//         };
//         let mut buf = Vec::new();
//         let mut stack = Vec::new();
//         info!("begin parsing");
//         loop {
//             match reader.read_event_into(&mut buf)? {
//                 Event::Start(tag) => {
//                     let tag_name: String = String::from_utf8(tag.name().as_ref().to_vec())?;
//                     trace!("'{tag_name}' open tag");
//                     match tag_name.as_str() {
//                         Self::SPECIFICATION if stack.is_empty() => {
//                             stack.push(ConvinceTag::Specification);
//                         }
//                         Self::MODEL
//                             if stack
//                                 .last()
//                                 .is_some_and(|tag| *tag == ConvinceTag::Specification) =>
//                         {
//                             stack.push(ConvinceTag::Model);
//                         }
//                         Self::PROPERTIES
//                             if stack
//                                 .last()
//                                 .is_some_and(|tag| *tag == ConvinceTag::Specification) =>
//                         {
//                             stack.push(ConvinceTag::Properties);
//                         }
//                         Self::COMPONENT_LIST
//                             if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
//                         {
//                             stack.push(ConvinceTag::ComponentList);
//                         }
//                         Self::BLACK_BOARD
//                             if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
//                         {
//                             stack.push(ConvinceTag::BlackBoard);
//                         }
//                         Self::SKILL_LIST
//                             if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
//                         {
//                             stack.push(ConvinceTag::SkillList);
//                         }
//                         Self::SKILL_DECLARATION
//                             if stack
//                                 .last()
//                                 .is_some_and(|tag| *tag == ConvinceTag::SkillList) =>
//                         {
//                             parser.parse_skill_declaration(tag, reader)?;
//                             // stack.push(ConvinceTag::SkillDeclaration);
//                         }
//                         Self::SKILL_DEFINITION =>
//                             // if stack
//                             //     .last()
//                             //     .is_some_and(|tag| *tag == ConvinceTag::SkillDeclaration) =>
//                         {
//                             parser.parse_skill_definition(tag, reader)?;
//                             stack.push(ConvinceTag::SkillDefinition);
//                         }
//                         Self::BT_TO_SKILL_INTERFACE
//                             if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
//                         {
//                             stack.push(ConvinceTag::BtToSkillInterface);
//                         }
//                         Self::BT if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) => {
//                             parser.parse_bt(tag)?;
//                             stack.push(ConvinceTag::Bt);
//                         }
//                         Self::COMPONENT
//                             if stack
//                                 .last()
//                                 .is_some_and(|tag| *tag == ConvinceTag::ComponentList) =>
//                         {
//                             parser.parse_component(tag)?;
//                             stack.push(ConvinceTag::Component);
//                         }
//                         Self::STRUCT_LIST
//                             if stack
//                                 .last()
//                                 .is_some_and(|tag| *tag == ConvinceTag::Component) =>
//                         {
//                             stack.push(ConvinceTag::StructList);
//                         }
//                         Self::ENUMERATION
//                             if stack
//                                 .last()
//                                 .is_some_and(|tag| *tag == ConvinceTag::Component) =>
//                         {
//                             stack.push(ConvinceTag::Enumeration);
//                         }
//                         Self::SERVICE
//                             if stack
//                                 .last()
//                                 .is_some_and(|tag| *tag == ConvinceTag::Component) =>
//                         {
//                             stack.push(ConvinceTag::Service);
//                         }
//                         Self::STRUCT
//                             if stack
//                                 .last()
//                                 .is_some_and(|tag| *tag == ConvinceTag::StructList) =>
//                         {
//                             parser.parse_struct(tag)?;
//                             stack.push(ConvinceTag::Struct);
//                         }
//                         Self::STRUCT_DATA
//                             if stack.last().is_some_and(|tag| *tag == ConvinceTag::Struct) =>
//                         {
//                             parser.parse_structdata(tag)?;
//                             stack.push(ConvinceTag::StructData);
//                         }
//                         Self::ENUM
//                             if stack
//                                 .last()
//                                 .is_some_and(|tag| *tag == ConvinceTag::Enumeration) =>
//                         {
//                             stack.push(ConvinceTag::Enum);
//                         }
//                         Self::FUNCTION
//                             if stack.last().is_some_and(|tag| *tag == ConvinceTag::Service) =>
//                         {
//                             stack.push(ConvinceTag::Function);
//                         }
//                         // Unknown tag: skip till maching end tag
//                         tag_name => {
//                             warn!("unknown or unexpected tag {tag_name}, skipping");
//                             reader.read_to_end_into(tag.to_end().into_owned().name(), &mut buf)?;
//                         }
//                     }
//                 }
//                 // exits the loop when reaching end of file
//                 Event::Eof => {
//                     info!("parsing completed");
//                     let model = parser.model.build();
//                     return Ok(model);
//                 }
//                 Event::End(tag) => {
//                     let name = tag.name();
//                     let name = str::from_utf8(name.as_ref())?;
//                     if stack.pop().is_some_and(|tag| <String>::from(tag) == name) {
//                         trace!("'{name}' end tag");
//                     } else {
//                         error!("unexpected end tag {name}");
//                         return Err(anyhow::Error::new(ParserError(
//                             reader.buffer_position(),
//                             ParserErrorType::UnexpectedEndTag(name.to_string()),
//                         )));
//                     }
//                 }
//                 Event::Empty(tag) => warn!("skipping empty tag"),
//                 Event::Text(_) => warn!("skipping text"),
//                 Event::Comment(_) => warn!("skipping comment"),
//                 Event::CData(_) => todo!(),
//                 Event::Decl(tag) => parser.parse_xml_declaration(tag)?,
//                 Event::PI(_) => todo!(),
//                 Event::DocType(_) => todo!(),
//             }
//             // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
//             buf.clear();
//         }
//     }

//     fn parse_xml_declaration(&self, tag: events::BytesDecl<'_>) -> Result<(), ParserError> {
//         // TODO: parse attributes
//         Ok(())
//     }

//     fn parse_skill_declaration<R: BufRead>(
//         &mut self,
//         tag: events::BytesStart,
//         reader: &Reader<R>,
//     ) -> anyhow::Result<()> {
//         for attr in tag
//             .attributes()
//             .into_iter()
//             .collect::<Result<Vec<Attribute>, AttrError>>()?
//         {
//             match str::from_utf8(attr.key.as_ref())? {
//                 Self::ID => {
//                     let skill_id = str::from_utf8(attr.value.as_ref())?;
//                     if !self.skills.contains_key(skill_id) {
//                         let pg_id = self.model.new_program_graph();
//                         self.skills.insert(skill_id.to_string(), pg_id);
//                     }
//                 }
//                 Self::INTERFACE => warn!("ignoring interface for now"),
//                 key => {
//                     error!(
//                         "found unknown attribute {key} in {}",
//                         Self::SKILL_DECLARATION
//                     );
//                     return Err(anyhow::Error::new(ParserError(
//                         reader.buffer_position(),
//                         ParserErrorType::UnknownKey(key.to_owned()),
//                     )));
//                 }
//             }
//         }
//         Ok(())
//     }

//     fn parse_skill_definition<R: BufRead>(
//         &mut self,
//         tag: events::BytesStart,
//         reader: &Reader<R>,
//     ) -> anyhow::Result<()> {
//         let mut moc = None;
//         let mut path = None;
//         for attr in tag
//             .attributes()
//             .into_iter()
//             .collect::<Result<Vec<Attribute>, AttrError>>()?
//         {
//             match str::from_utf8(attr.key.as_ref())? {
//                 Self::TYPE => {
//                     todo!()
//                 }
//                 Self::MOC => moc = Some(String::from_utf8(attr.value.into_owned())?),
//                 Self::PATH => {
//                     path = Some(PathBuf::try_from(String::from_utf8(
//                         attr.value.into_owned(),
//                     )?)?)
//                 }
//                 key => {
//                     error!(
//                         "found unknown attribute {key} in {}",
//                         Self::SKILL_DECLARATION
//                     );
//                     return Err(anyhow::Error::new(ParserError(
//                         reader.buffer_position(),
//                         ParserErrorType::UnknownKey(key.to_owned()),
//                     )));
//                 }
//             }
//         }
//         let path = path.ok_or(ParserError(
//             reader.buffer_position(),
//             ParserErrorType::MissingAttr(Self::PATH.to_string()),
//         ))?;
//         info!("creating reader from file {path:?}");
//         let mut reader = Reader::from_file(path)?;
//         match moc.as_deref() {
//             Some(Self::FSM) => {
//                 self.parse_skill(&mut reader)?;
//             }
//             Some(Self::BT) => {
//                 self.parse_skill(&mut reader)?;
//             }
//             Some(_) => {
//                 error!("unrecognized moc");
//             }
//             None => {
//                 error!("missing attribute moc");
//             }
//         }
//         Ok(())
//     }

//     fn parse_properties<R: BufRead>(
//         &mut self,
//         tag: &events::BytesStart,
//         reader: &mut Reader<R>,
//     ) -> Result<(), ParserError> {
//         todo!()
//     }

//     fn parse_datamodel<R: BufRead>(
//         &mut self,
//         _tag: &events::BytesStart,
//         reader: &mut Reader<R>,
//         pg_id: PgId,
//     ) -> Result<(), ParserError> {
//         let mut buf = Vec::new();
//         loop {
//             match reader.read_event_into(&mut buf).map_err(|err| {
//                 ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
//             })? {
//                 Event::Empty(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                 })? {
//                     Self::DATA => self.parse_data(reader, &tag, pg_id)?,
//                     tag_name => warn!("unknown empty tag {tag_name}, skipping"),
//                 },
//                 Event::Start(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
//                     ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                 })? {
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
//                     Self::DATAMODEL => return Ok(()),
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

//     fn parse_data<R: BufRead>(
//         &mut self,
//         reader: &mut Reader<R>,
//         tag: &events::BytesStart<'_>,
//         pg_id: PgId,
//     ) -> Result<(), ParserError> {
//         let mut id = None;
//         let mut var_type = VarType::Unit;
//         // let mut value = None;
//         for attr in tag.attributes() {
//             let attr = attr
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Attr(err)))?;
//             match str::from_utf8(attr.key.as_ref())
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Utf8(err)))?
//             {
//                 Self::ID => {
//                     id = Some(String::from_utf8(attr.value.to_vec()).map_err(|err| {
//                         ParserError(
//                             reader.buffer_position(),
//                             ParserErrorType::Utf8(err.utf8_error()),
//                         )
//                     })?);
//                 }
//                 Self::TYPE => {
//                     match str::from_utf8(attr.value.as_ref()).map_err(|err| {
//                         ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
//                     })? {
//                         Self::BOOL => var_type = VarType::Boolean,
//                         Self::INT => var_type = VarType::Integer,
//                         Self::UNIT => var_type = VarType::Unit,
//                         _ => error!("unknown data type, ignoring"),
//                     }
//                 }
//                 name => warn!("unknown attribute {name}, ignoring"),
//             }
//         }
//         if let Some(id) = id {
//             let val_id = self
//                 .model
//                 .new_var(pg_id, var_type)
//                 .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Cs(err)))?;
//             self.vars.insert(id, val_id);
//         } else {
//             todo!()
//         }
//         Ok(())
//     }

//     // fn parse_script<R: BufRead>(
//     //     &self,
//     //     tag: events::BytesStart,
//     //     reader: &mut Reader<R>,
//     //     pg_id: PgId,
//     // ) -> Result<(), ParserError> {
//     //     todo!()
//     // }

//     fn parse_bt(
//         &self,
//         // reader: &mut Reader<R>,
//         tag: events::BytesStart<'_>,
//     ) -> anyhow::Result<()> {
//         for attr in tag.attributes() {
//             let attr = attr?;
//             match str::from_utf8(attr.key.as_ref())? {
//                 Self::FILE => {
//                     let file = str::from_utf8(attr.value.as_ref())?;
//                     let file = PathBuf::try_from(file)?;
//                     todo!()
//                 }
//                 name => error!("unknown attribute {name}, ignoring"),
//             }
//         }
//         Ok(())
//     }

//     fn parse_component(&self, tag: events::BytesStart<'_>) -> anyhow::Result<()> {
//         for attr in tag.attributes() {
//             let attr = attr?;
//             match str::from_utf8(attr.key.as_ref())? {
//                 Self::ID => {
//                     let id = str::from_utf8(attr.value.as_ref())?;
//                     todo!()
//                 }
//                 name => error!("unknown attribute {name}, ignoring"),
//             }
//         }
//         Ok(())
//     }

//     fn parse_struct(&self, tag: events::BytesStart<'_>) -> anyhow::Result<()> {
//         for attr in tag.attributes() {
//             let attr = attr?;
//             match str::from_utf8(attr.key.as_ref())? {
//                 Self::ID => {
//                     let id = str::from_utf8(attr.value.as_ref())?;
//                     todo!()
//                 }
//                 name => error!("unknown attribute {name}, ignoring"),
//             }
//         }
//         Ok(())
//     }

//     fn parse_structdata(&self, tag: events::BytesStart<'_>) -> anyhow::Result<()> {
//         for attr in tag.attributes() {
//             let attr = attr?;
//             match str::from_utf8(attr.key.as_ref())? {
//                 Self::FIELD_ID => {
//                     let field_id = str::from_utf8(attr.value.as_ref())?;
//                     let field_id = field_id.parse::<usize>()?;
//                     todo!()
//                 }
//                 name => error!("unknown attribute {name}, ignoring"),
//             }
//         }
//         Ok(())
//     }
// }
