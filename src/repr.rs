use std::collections::HashMap;

pub struct Specification {
    pub model: Model,
    // properties: Vec<Property>,
    // property_id: HashMap<String, PropertyId>,
}

pub struct SkillId(usize);

pub struct Model {
    pub task_plan: SkillId,
    pub skill_list: Vec<SkillDeclaration>,
    pub skill_id: HashMap<String, SkillId>,
    // blackboard: Blackboard,
    // component_list: Vec<Component>,
    // component_id: HashMap<String, ComponentId>,
    // interface_list: Vec<Interface>,
    // interface_id: HashMap<String, InterfaceId>,
}

pub enum SkillType {
    Action,
    Condition,
}

pub enum MoC {
    Fsm,
    Bt,
}

pub struct SkillDeclaration {
    // interface: InterfaceId,
    pub skill_type: SkillType,
    pub moc: MoC,
    // path: PathBuf,
}
