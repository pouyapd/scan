mod bt;
mod fsm;

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::io::BufRead;
use std::path::PathBuf;
use std::str;
use std::str::Utf8Error;

use anyhow::anyhow;
use log::{error, info, trace, warn};
use quick_xml::events::attributes::{AttrError, Attribute};
use quick_xml::{events, Error as XmlError};
use quick_xml::{events::Event, Reader};

use crate::model::{
    ChannelSystem, ChannelSystemBuilder, CsAction, CsError, CsLocation, CsVar, PgId, VarType,
};

#[derive(Debug)]
pub enum ParserErrorType {
    Reader(XmlError),
    UnknownEvent(Event<'static>),
    Attr(AttrError),
    UnknownKey(String),
    Utf8(Utf8Error),
    Cs(CsError),
    UnexpectedEndTag(String),
    MissingLocation,
    UnknownVar(String),
    MissingExpr,
    UnexpectedStartTag(String),
    MissingAttr(String),
    MissingSkill(SkillId),
    UnclosedTags,
    AlreadyDeclared(String),
    UnknownMoC(String),
    UnknownSkillType(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillId(usize);
// pub struct ComponentId(usize);
// pub struct PropertyId(usize);

#[derive(Debug)]
pub struct Specification {
    task_plan: Option<SkillId>,
    skill_list: Vec<(String, Option<SkillDeclaration>)>,
    skill_id: HashMap<String, SkillId>,
    // blackboard: Blackboard,
    // component_list: Vec<Component>,
    // component_id: HashMap<String, ComponentId>,
    // interface_list: Vec<Interface>,
    // interface_id: HashMap<String, InterfaceId>,
    // properties: Vec<Property>,
    // property_id: HashMap<String, PropertyId>,
}

#[derive(Debug)]
pub enum SkillType {
    Action,
    Condition,
}

impl TryFrom<String> for SkillType {
    type Error = ParserErrorType;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "action" => Ok(SkillType::Action),
            "condition" => Ok(SkillType::Condition),
            _ => Err(ParserErrorType::UnknownSkillType(value)),
        }
    }
}

#[derive(Debug)]
pub enum MoC {
    Fsm,
    Bt,
}

impl TryFrom<String> for MoC {
    type Error = ParserErrorType;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "fsm" => Ok(MoC::Fsm),
            "bt" => Ok(MoC::Bt),
            _ => Err(ParserErrorType::UnknownMoC(value)),
        }
    }
}

#[derive(Debug)]
pub struct SkillDeclaration {
    // interface: InterfaceId,
    skill_type: SkillType,
    moc: MoC,
    path: PathBuf,
}

impl Specification {
    pub fn parse<R: BufRead>(reader: &mut Reader<R>) -> anyhow::Result<Specification> {
        let mut spec = Specification {
            task_plan: None,
            skill_list: Vec::new(),
            skill_id: HashMap::new(),
        };
        let mut buf = Vec::new();
        let mut stack = Vec::new();
        info!("begin parsing");
        loop {
            match reader.read_event_into(&mut buf)? {
                Event::Start(tag) => {
                    let tag_name: String = String::from_utf8(tag.name().as_ref().to_vec())?;
                    trace!("'{tag_name}' open tag");
                    let tag_name = ConvinceTag::from(tag_name.as_str());
                    match tag_name {
                        ConvinceTag::Specification if stack.is_empty() => {
                            stack.push(ConvinceTag::Specification);
                        }
                        ConvinceTag::Model
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::Specification) =>
                        {
                            stack.push(ConvinceTag::Model);
                        }
                        ConvinceTag::SkillList
                            if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
                        {
                            spec.parse_skill_list(tag, reader)?;
                            stack.push(ConvinceTag::SkillList);
                        }
                        ConvinceTag::SkillDeclaration { .. }
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::SkillList) =>
                        {
                            let skill_id = spec.parse_skill_declaration(tag, reader)?;
                            if !spec.skill_id.contains_key(&skill_id) {
                                let idx = SkillId(spec.skill_list.len());
                                spec.skill_id.insert(skill_id.to_string(), idx);
                                spec.skill_list.push((skill_id.clone(), None));
                            }
                            stack.push(ConvinceTag::SkillDeclaration { skill_id });
                        }
                        ConvinceTag::Properties
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::Specification) =>
                        {
                            stack.push(ConvinceTag::Properties);
                        }
                        ConvinceTag::ComponentList
                            if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
                        {
                            stack.push(ConvinceTag::ComponentList);
                        }
                        ConvinceTag::Component
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::ComponentList) =>
                        {
                            // spec.parse_component(tag)?;
                            stack.push(ConvinceTag::Component);
                        }
                        ConvinceTag::Function
                            if stack.last().is_some_and(|tag| *tag == ConvinceTag::Service) =>
                        {
                            stack.push(ConvinceTag::Function);
                        }
                        // Unknown tag: skip till maching end tag
                        ConvinceTag::Unknown(_) | _ => {
                            // warn!("unknown or unexpected tag {tag_name}, skipping");
                            reader.read_to_end_into(tag.to_end().into_owned().name(), &mut buf)?;
                        }
                    }
                }
                Event::End(tag) => {
                    let name = tag.name();
                    let name = str::from_utf8(name.as_ref())?;
                    if stack.pop().is_some_and(|tag| <String>::from(tag) == name) {
                        trace!("'{name}' end tag");
                    } else {
                        error!("unexpected end tag {name}");
                        return Err(anyhow::Error::new(ParserError(
                            reader.buffer_position(),
                            ParserErrorType::UnexpectedEndTag(name.to_string()),
                        )));
                    }
                }
                Event::Empty(tag) => {
                    let tag_name: String = String::from_utf8(tag.name().as_ref().to_vec())?;
                    trace!("'{tag_name}' empty tag");
                    let tag_name = ConvinceTag::from(tag_name.as_str());
                    match tag_name {
                        ConvinceTag::SkillDefinition
                            if matches!(
                                stack.last(),
                                Some(ConvinceTag::SkillDeclaration { .. })
                            ) =>
                        {
                            if let Some(ConvinceTag::SkillDeclaration { skill_id }) = stack.last() {
                                spec.parse_skill_definition(skill_id.to_owned(), tag, reader)?;
                            } else {
                                panic!("match guard prevents this");
                            }
                        }
                        // Unknown tag: skip till maching end tag
                        ConvinceTag::Unknown(_) | _ => {
                            warn!("unknown or unexpected tag {tag_name:?}, skipping");
                            continue;
                        }
                    }
                }
                Event::Text(_) => warn!("skipping text"),
                Event::Comment(_) => warn!("skipping comment"),
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
        const TASK_PLAN: &str = "taskPlan";
        for attr in tag
            .attributes()
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                TASK_PLAN => {
                    let skill_id = str::from_utf8(attr.value.as_ref())?;
                    if self.skill_id.contains_key(skill_id) {
                        // This means 'taskPlan' tag comes after declaration
                        // Should not happen?
                        return Err(anyhow!(ParserError(
                            reader.buffer_position(),
                            ParserErrorType::AlreadyDeclared(skill_id.to_string()),
                        )));
                        // self.task_plan = Some(*idx);
                    } else {
                        let idx = SkillId(self.skill_list.len());
                        self.skill_id.insert(skill_id.to_string(), idx);
                        self.skill_list.push((skill_id.to_string(), None));
                        self.task_plan = Some(idx);
                    }
                }
                key => {
                    error!("found unknown attribute {key} in {TASK_PLAN}",);
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
    ) -> anyhow::Result<String> {
        const ID: &str = "id";
        const INTERFACE: &str = "interface";
        let mut skill_id: Option<String> = None;
        let mut interface: Option<String> = None;
        for attr in tag
            .attributes()
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                ID => {
                    skill_id = Some(String::from_utf8(attr.value.into_owned())?);
                }
                INTERFACE => {
                    interface = Some(String::from_utf8(attr.value.into_owned())?);
                }
                key => {
                    error!("found unknown attribute {key} in {}", INTERFACE);
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        skill_id.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(ID.to_string())
        )))
    }

    fn parse_skill_definition<R: BufRead>(
        &mut self,
        skill_id: String,
        tag: events::BytesStart<'_>,
        reader: &mut Reader<R>,
    ) -> anyhow::Result<()> {
        const TYPE: &str = "type";
        const MOC: &str = "moc";
        const PATH: &str = "path";
        let mut type_skill: Option<String> = None;
        let mut moc: Option<String> = None;
        let mut path: Option<String> = None;
        for attr in tag
            .attributes()
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                TYPE => {
                    type_skill = Some(String::from_utf8(attr.value.into_owned())?);
                }
                MOC => {
                    moc = Some(String::from_utf8(attr.value.into_owned())?);
                }
                PATH => {
                    path = Some(String::from_utf8(attr.value.into_owned())?);
                }
                key => {
                    error!("found unknown attribute {key} in skillDefinition");
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        let skill_type = type_skill.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(TYPE.to_string())
        )))?;
        let skill_type = SkillType::try_from(skill_type)?;
        let moc = moc.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(MOC.to_string())
        )))?;
        let moc = MoC::try_from(moc)?;
        let path = path.ok_or(anyhow!(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(PATH.to_string())
        )))?;
        let path = PathBuf::from(path);
        let skill = SkillDeclaration {
            skill_type,
            moc,
            path,
        };
        let idx = self
            .skill_id
            .get(&skill_id)
            .expect("skill_id was already added");
        *self.skill_list.get_mut(idx.0).ok_or_else(|| {
            anyhow!(ParserError(
                reader.buffer_position(),
                ParserErrorType::MissingSkill(*idx)
            ))
        })? = (skill_id, Some(skill));
        Ok(())
    }
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
            ParserErrorType::UnexpectedStartTag(_) => todo!(),
            ParserErrorType::UnexpectedEndTag(_) => write!(f, "self:#?"),
            ParserErrorType::MissingLocation => todo!(),
            ParserErrorType::UnknownVar(_) => todo!(),
            ParserErrorType::MissingExpr => todo!(),
            ParserErrorType::MissingAttr(_) => todo!(),
            ParserErrorType::UnclosedTags => todo!(),
            ParserErrorType::AlreadyDeclared(_) => todo!(),
            _ => todo!(),
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
pub struct ParserError(pub(crate) usize, pub(crate) ParserErrorType);

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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConvinceTag {
    Specification,
    Model,
    Properties,
    Scxml,
    ComponentList,
    BlackBoard,
    SkillList,
    BtToSkillInterface,
    Bt,
    Component,
    SkillDeclaration { skill_id: String },
    SkillDefinition,
    StructList,
    Enumeration,
    Service,
    Struct,
    StructData,
    Enum,
    Function,
    Unknown(String),
}

impl From<ConvinceTag> for String {
    fn from(value: ConvinceTag) -> Self {
        match value {
            ConvinceTag::Specification => "specification".to_string(),
            ConvinceTag::Model => "model".to_string(),
            ConvinceTag::Properties => "properties".to_string(),
            ConvinceTag::Scxml => "scxml".to_string(),
            ConvinceTag::ComponentList => "componentList".to_string(),
            ConvinceTag::BlackBoard => "blackBoard".to_string(),
            ConvinceTag::SkillList => "skillList".to_string(),
            ConvinceTag::BtToSkillInterface => "btBoSkillInterface".to_string(),
            ConvinceTag::Bt => "bt".to_string(),
            ConvinceTag::Component => "component".to_string(),
            ConvinceTag::SkillDeclaration { .. } => "skillDeclaration".to_string(),
            ConvinceTag::SkillDefinition => "skillDefinition".to_string(),
            ConvinceTag::StructList => "stuctList".to_string(),
            ConvinceTag::Enumeration => "enumeration".to_string(),
            ConvinceTag::Service => "service".to_string(),
            ConvinceTag::Struct => "struct".to_string(),
            ConvinceTag::StructData => "struct_data".to_string(),
            ConvinceTag::Enum => "enum".to_string(),
            ConvinceTag::Function => "function".to_string(),
            ConvinceTag::Unknown(tag) => tag,
        }
    }
}

impl From<&str> for ConvinceTag {
    fn from(value: &str) -> Self {
        match value {
            "specification" => ConvinceTag::Specification,
            "model" => ConvinceTag::Model,
            "properties" => ConvinceTag::Properties,
            "scxml" => ConvinceTag::Scxml,
            "componentList" => ConvinceTag::ComponentList,
            "blackBoard" => ConvinceTag::BlackBoard,
            "skillList" => ConvinceTag::SkillList,
            "btBoSkillInterface" => ConvinceTag::BtToSkillInterface,
            "bt" => ConvinceTag::Bt,
            "component" => ConvinceTag::Component,
            "skillDeclaration" => ConvinceTag::SkillDeclaration {
                skill_id: String::new(),
            },
            "skillDefinition" => ConvinceTag::SkillDefinition,
            "stuctList" => ConvinceTag::StructList,
            "enumeration" => ConvinceTag::Enumeration,
            "service" => ConvinceTag::Service,
            "struct" => ConvinceTag::Struct,
            "struct_data" => ConvinceTag::StructData,
            "enum" => ConvinceTag::Enum,
            "function" => ConvinceTag::Function,
            tag => ConvinceTag::Unknown(tag.to_string()),
        }
    }
}

#[derive(Debug)]
pub struct Parser {
    model: ChannelSystemBuilder,
    skills: HashMap<String, PgId>,
    states: HashMap<String, CsLocation>,
    events: HashMap<String, CsAction>,
    vars: HashMap<String, CsVar>,
}

impl Parser {
    const SPECIFICATION: &'static str = "specification";
    const MODEL: &'static str = "model";
    const PROPERTIES: &'static str = "properties";
    const COMPONENT_LIST: &'static str = "componentList";
    const BLACK_BOARD: &'static str = "blackBoard";
    const SKILL_LIST: &'static str = "skillList";
    const BT_TO_SKILL_INTERFACE: &'static str = "btToSkillInterface";
    const BT: &'static str = "bt";
    const COMPONENT: &'static str = "component";
    const SKILL_DECLARATION: &'static str = "skillDeclaration";
    const SKILL_DEFINITION: &'static str = "skillDefinition";
    const FILE: &'static str = "file";
    const STATE: &'static str = "state";
    const SCXML: &'static str = "scxml";
    const INITIAL: &'static str = "initial";
    const ID: &'static str = "id";
    const MOC: &'static str = "moc";
    const PATH: &'static str = "path";
    const FSM: &'static str = "fsm";
    const INTERFACE: &'static str = "interface";
    const VERSION: &'static str = "version";
    const NAME: &'static str = "name";
    const XMLNS: &'static str = "xmlns";
    const DATAMODEL: &'static str = "datamodel";
    const DATA: &'static str = "data";
    const TYPE: &'static str = "type";
    const BOOL: &'static str = "bool";
    const INT: &'static str = "int";
    const UNIT: &'static str = "unit";
    const BINDING: &'static str = "binding";
    const TRANSITION: &'static str = "transition";
    const TARGET: &'static str = "target";
    const EVENT: &'static str = "event";
    const ON_ENTRY: &'static str = "onentry";
    const ON_EXIT: &'static str = "onexit";
    const NULL: &'static str = "NULL";
    const SCRIPT: &'static str = "script";
    const ASSIGN: &'static str = "assign";
    const LOCATION: &'static str = "location";
    const EXPR: &'static str = "expr";
    const RAISE: &'static str = "raise";
    const STRUCT_LIST: &'static str = "structList";
    const ENUMERATION: &'static str = "enumeration";
    const SERVICE: &'static str = "service";
    const STRUCT: &'static str = "struct";
    const STRUCT_DATA: &'static str = "structData";
    const FIELD_ID: &'static str = "fieldId";
    const ENUM: &'static str = "enum";
    const FUNCTION: &'static str = "function";

    pub fn parse<R: BufRead>(reader: &mut Reader<R>) -> anyhow::Result<ChannelSystem> {
        let mut parser = Self {
            model: ChannelSystemBuilder::default(),
            skills: HashMap::default(),
            states: HashMap::default(),
            events: HashMap::default(),
            vars: HashMap::default(),
        };
        let mut buf = Vec::new();
        let mut stack = Vec::new();
        info!("begin parsing");
        loop {
            match reader.read_event_into(&mut buf)? {
                Event::Start(tag) => {
                    let tag_name: String = String::from_utf8(tag.name().as_ref().to_vec())?;
                    trace!("'{tag_name}' open tag");
                    match tag_name.as_str() {
                        Self::SPECIFICATION if stack.is_empty() => {
                            stack.push(ConvinceTag::Specification);
                        }
                        Self::MODEL
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::Specification) =>
                        {
                            stack.push(ConvinceTag::Model);
                        }
                        Self::PROPERTIES
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::Specification) =>
                        {
                            stack.push(ConvinceTag::Properties);
                        }
                        Self::COMPONENT_LIST
                            if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
                        {
                            stack.push(ConvinceTag::ComponentList);
                        }
                        Self::BLACK_BOARD
                            if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
                        {
                            stack.push(ConvinceTag::BlackBoard);
                        }
                        Self::SKILL_LIST
                            if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
                        {
                            stack.push(ConvinceTag::SkillList);
                        }
                        Self::SKILL_DECLARATION
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::SkillList) =>
                        {
                            parser.parse_skill_declaration(tag, reader)?;
                            // stack.push(ConvinceTag::SkillDeclaration);
                        }
                        Self::SKILL_DEFINITION =>
                            // if stack
                            //     .last()
                            //     .is_some_and(|tag| *tag == ConvinceTag::SkillDeclaration) =>
                        {
                            parser.parse_skill_definition(tag, reader)?;
                            stack.push(ConvinceTag::SkillDefinition);
                        }
                        Self::BT_TO_SKILL_INTERFACE
                            if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) =>
                        {
                            stack.push(ConvinceTag::BtToSkillInterface);
                        }
                        Self::BT if stack.last().is_some_and(|tag| *tag == ConvinceTag::Model) => {
                            parser.parse_bt(tag)?;
                            stack.push(ConvinceTag::Bt);
                        }
                        Self::COMPONENT
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::ComponentList) =>
                        {
                            parser.parse_component(tag)?;
                            stack.push(ConvinceTag::Component);
                        }
                        Self::STRUCT_LIST
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::Component) =>
                        {
                            stack.push(ConvinceTag::StructList);
                        }
                        Self::ENUMERATION
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::Component) =>
                        {
                            stack.push(ConvinceTag::Enumeration);
                        }
                        Self::SERVICE
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::Component) =>
                        {
                            stack.push(ConvinceTag::Service);
                        }
                        Self::STRUCT
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::StructList) =>
                        {
                            parser.parse_struct(tag)?;
                            stack.push(ConvinceTag::Struct);
                        }
                        Self::STRUCT_DATA
                            if stack.last().is_some_and(|tag| *tag == ConvinceTag::Struct) =>
                        {
                            parser.parse_structdata(tag)?;
                            stack.push(ConvinceTag::StructData);
                        }
                        Self::ENUM
                            if stack
                                .last()
                                .is_some_and(|tag| *tag == ConvinceTag::Enumeration) =>
                        {
                            stack.push(ConvinceTag::Enum);
                        }
                        Self::FUNCTION
                            if stack.last().is_some_and(|tag| *tag == ConvinceTag::Service) =>
                        {
                            stack.push(ConvinceTag::Function);
                        }
                        // Unknown tag: skip till maching end tag
                        tag_name => {
                            warn!("unknown or unexpected tag {tag_name}, skipping");
                            reader.read_to_end_into(tag.to_end().into_owned().name(), &mut buf)?;
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
                    let name = tag.name();
                    let name = str::from_utf8(name.as_ref())?;
                    if stack.pop().is_some_and(|tag| <String>::from(tag) == name) {
                        trace!("'{name}' end tag");
                    } else {
                        error!("unexpected end tag {name}");
                        return Err(anyhow::Error::new(ParserError(
                            reader.buffer_position(),
                            ParserErrorType::UnexpectedEndTag(name.to_string()),
                        )));
                    }
                }
                Event::Empty(tag) => warn!("skipping empty tag"),
                Event::Text(_) => warn!("skipping text"),
                Event::Comment(_) => warn!("skipping comment"),
                Event::CData(_) => todo!(),
                Event::Decl(tag) => parser.parse_xml_declaration(tag)?,
                Event::PI(_) => todo!(),
                Event::DocType(_) => todo!(),
            }
            // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
            buf.clear();
        }
    }

    fn parse_xml_declaration(&self, tag: events::BytesDecl<'_>) -> Result<(), ParserError> {
        // TODO: parse attributes
        Ok(())
    }

    fn parse_skill_declaration<R: BufRead>(
        &mut self,
        tag: events::BytesStart,
        reader: &Reader<R>,
    ) -> anyhow::Result<()> {
        for attr in tag
            .attributes()
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                Self::ID => {
                    let skill_id = str::from_utf8(attr.value.as_ref())?;
                    if !self.skills.contains_key(skill_id) {
                        let pg_id = self.model.new_program_graph();
                        self.skills.insert(skill_id.to_string(), pg_id);
                    }
                }
                Self::INTERFACE => warn!("ignoring interface for now"),
                key => {
                    error!(
                        "found unknown attribute {key} in {}",
                        Self::SKILL_DECLARATION
                    );
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        Ok(())
    }

    fn parse_skill_definition<R: BufRead>(
        &mut self,
        tag: events::BytesStart,
        reader: &Reader<R>,
    ) -> anyhow::Result<()> {
        let mut moc = None;
        let mut path = None;
        for attr in tag
            .attributes()
            .into_iter()
            .collect::<Result<Vec<Attribute>, AttrError>>()?
        {
            match str::from_utf8(attr.key.as_ref())? {
                Self::TYPE => {
                    todo!()
                }
                Self::MOC => moc = Some(String::from_utf8(attr.value.into_owned())?),
                Self::PATH => {
                    path = Some(PathBuf::try_from(String::from_utf8(
                        attr.value.into_owned(),
                    )?)?)
                }
                key => {
                    error!(
                        "found unknown attribute {key} in {}",
                        Self::SKILL_DECLARATION
                    );
                    return Err(anyhow::Error::new(ParserError(
                        reader.buffer_position(),
                        ParserErrorType::UnknownKey(key.to_owned()),
                    )));
                }
            }
        }
        let path = path.ok_or(ParserError(
            reader.buffer_position(),
            ParserErrorType::MissingAttr(Self::PATH.to_string()),
        ))?;
        info!("creating reader from file {path:?}");
        let mut reader = Reader::from_file(path)?;
        match moc.as_deref() {
            Some(Self::FSM) => {
                self.parse_skill(&mut reader)?;
            }
            Some(Self::BT) => {
                self.parse_skill(&mut reader)?;
            }
            Some(_) => {
                error!("unrecognized moc");
            }
            None => {
                error!("missing attribute moc");
            }
        }
        Ok(())
    }

    fn parse_properties<R: BufRead>(
        &mut self,
        tag: &events::BytesStart,
        reader: &mut Reader<R>,
    ) -> Result<(), ParserError> {
        todo!()
    }

    fn parse_datamodel<R: BufRead>(
        &mut self,
        _tag: &events::BytesStart,
        reader: &mut Reader<R>,
        pg_id: PgId,
    ) -> Result<(), ParserError> {
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf).map_err(|err| {
                ParserError(reader.buffer_position(), ParserErrorType::Reader(err))
            })? {
                Event::Empty(tag) => match str::from_utf8(tag.name().as_ref()).map_err(|err| {
                    ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                })? {
                    Self::DATA => self.parse_data(reader, &tag, pg_id)?,
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
                    Self::DATAMODEL => return Ok(()),
                    name => {
                        error!("unexpected end tag {name}");
                        return Err(ParserError(
                            reader.buffer_position(),
                            ParserErrorType::UnexpectedEndTag(name.to_string()),
                        ));
                    }
                },
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

    fn parse_data<R: BufRead>(
        &mut self,
        reader: &mut Reader<R>,
        tag: &events::BytesStart<'_>,
        pg_id: PgId,
    ) -> Result<(), ParserError> {
        let mut id = None;
        let mut var_type = VarType::Unit;
        // let mut value = None;
        for attr in tag.attributes() {
            let attr = attr
                .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Attr(err)))?;
            match str::from_utf8(attr.key.as_ref())
                .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Utf8(err)))?
            {
                Self::ID => {
                    id = Some(String::from_utf8(attr.value.to_vec()).map_err(|err| {
                        ParserError(
                            reader.buffer_position(),
                            ParserErrorType::Utf8(err.utf8_error()),
                        )
                    })?);
                }
                Self::TYPE => {
                    match str::from_utf8(attr.value.as_ref()).map_err(|err| {
                        ParserError(reader.buffer_position(), ParserErrorType::Utf8(err))
                    })? {
                        Self::BOOL => var_type = VarType::Boolean,
                        Self::INT => var_type = VarType::Integer,
                        Self::UNIT => var_type = VarType::Unit,
                        _ => error!("unknown data type, ignoring"),
                    }
                }
                name => warn!("unknown attribute {name}, ignoring"),
            }
        }
        if let Some(id) = id {
            let val_id = self
                .model
                .new_var(pg_id, var_type)
                .map_err(|err| ParserError(reader.buffer_position(), ParserErrorType::Cs(err)))?;
            self.vars.insert(id, val_id);
        } else {
            todo!()
        }
        Ok(())
    }

    // fn parse_script<R: BufRead>(
    //     &self,
    //     tag: events::BytesStart,
    //     reader: &mut Reader<R>,
    //     pg_id: PgId,
    // ) -> Result<(), ParserError> {
    //     todo!()
    // }

    fn parse_bt(
        &self,
        // reader: &mut Reader<R>,
        tag: events::BytesStart<'_>,
    ) -> anyhow::Result<()> {
        for attr in tag.attributes() {
            let attr = attr?;
            match str::from_utf8(attr.key.as_ref())? {
                Self::FILE => {
                    let file = str::from_utf8(attr.value.as_ref())?;
                    let file = PathBuf::try_from(file)?;
                    todo!()
                }
                name => error!("unknown attribute {name}, ignoring"),
            }
        }
        Ok(())
    }

    fn parse_component(&self, tag: events::BytesStart<'_>) -> anyhow::Result<()> {
        for attr in tag.attributes() {
            let attr = attr?;
            match str::from_utf8(attr.key.as_ref())? {
                Self::ID => {
                    let id = str::from_utf8(attr.value.as_ref())?;
                    todo!()
                }
                name => error!("unknown attribute {name}, ignoring"),
            }
        }
        Ok(())
    }

    fn parse_struct(&self, tag: events::BytesStart<'_>) -> anyhow::Result<()> {
        for attr in tag.attributes() {
            let attr = attr?;
            match str::from_utf8(attr.key.as_ref())? {
                Self::ID => {
                    let id = str::from_utf8(attr.value.as_ref())?;
                    todo!()
                }
                name => error!("unknown attribute {name}, ignoring"),
            }
        }
        Ok(())
    }

    fn parse_structdata(&self, tag: events::BytesStart<'_>) -> anyhow::Result<()> {
        for attr in tag.attributes() {
            let attr = attr?;
            match str::from_utf8(attr.key.as_ref())? {
                Self::FIELD_ID => {
                    let field_id = str::from_utf8(attr.value.as_ref())?;
                    let field_id = field_id.parse::<usize>()?;
                    todo!()
                }
                name => error!("unknown attribute {name}, ignoring"),
            }
        }
        Ok(())
    }
}
