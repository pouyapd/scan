use anyhow::Result;
use anyhow::Context;
use anyhow::anyhow; // Removed unused anyhow::anyhow import

use crate::build_tree::tree;
use crate::build_parser::{Transition, Executable};

use log::{trace};

#[derive(Debug, Clone)]
pub struct State {
    pub(crate) id: String,
    pub(crate) transitions: Vec<Transition>,
    pub(crate) on_entry: Vec<Executable>,
    pub(crate) on_exit: Vec<Executable>,
}

impl State {
    pub fn new(id_c:String,transition_c:Vec<Transition>,on_entry_c:Vec<Executable>,on_exit_c:Vec<Executable>) -> Self {
        State{
            id:id_c,
            transitions : transition_c,
            on_entry : on_entry_c,
            on_exit : on_exit_c
        }
    }

    pub fn build_state(s:tree::Tree,
        interner: &mut boa_interner::Interner
    ) -> Result<State> {
        trace!(target: "parser", "start build state");

        let mut id_c ="".to_string();
        let mut transition_c: Vec<Transition> = Vec::new();
        let mut on_entry_c:Vec<Executable> = Vec::new();
        let mut on_exit_c:Vec<Executable> = Vec::new();

        let state_attributes = s.get_value().get_attribute_list();

        let mut _has_id = false; // Prefixed with _
        for atr in state_attributes {
            let cur_atr = atr.get_name();
            match cur_atr {
                "id" => {
                    id_c = atr.get_value().to_string();
                    _has_id = true;
                }
                _key => {}
            }
        }

        if id_c.is_empty() { // Changed from _has_id check to check the string itself
            anyhow::bail!("Missing required attribute 'id' in <state> element");
        }

        let state_children = s.get_children();

        for child in state_children {
            let cur_child_value = child.get_value();
            let cur_child_name = cur_child_value.get_name();

            match cur_child_name {
                "transition" =>{
                   let transition_instance = Transition::build_transition(child, interner)
                        .context("Failed to build transition")?;
                   transition_c.push(transition_instance);
                }
                "onentry" =>{
                    let cur_one_entry = State::build_on_entry(child, interner)
                         .context("Failed to build onentry content")?;
                    on_entry_c.extend(cur_one_entry);
                }
                "onexit" =>{
                    let cur_one_exit = State::build_on_exit(child, interner)
                         .context("Failed to build onexit content")?;
                    on_exit_c.extend(cur_one_exit);
                }
                _key => {}
            }
        }

        let state = State::new(id_c,transition_c,on_entry_c,on_exit_c);

        trace!(target: "parser", "end build state");
        Ok(state)
    }

    fn build_on_entry(s:tree::Tree,interner: &mut boa_interner::Interner)-> Result<Vec<Executable>> {
        trace!(target: "parser", "start build on_entry");
        let mut exec:Vec<Executable> = Vec::new();

        let on_entry_children = s.get_children();

        for child in on_entry_children {
            let cur_child_value = child.get_value();
            let cur_child_name = cur_child_value.get_name();

            let executable_instance = match cur_child_name {
                "raise" => Executable::build_execut_raise(child)
                    .context("Failed to build 'raise' executable in onentry")?,
                "assign" => Executable::build_execut_assign(child, interner)
                    .context("Failed to build 'assign' executable in onentry")?,
                "if" => Executable::build_execut_if(child, interner)
                    .context("Failed to build nested 'if' executable in onentry")?,
                "send" => Executable::build_execut_send(child, interner)
                    .context("Failed to build 'send' executable in onentry")?,
                _key => {
                     anyhow::bail!("Unexpected child element '{}' in <onentry> element", _key);
                }
            };
            exec.push(executable_instance);
        }
        trace!(target: "parser", "end build on_entry");
        Ok(exec)
    }

    fn build_on_exit(s:tree::Tree,interner: &mut boa_interner::Interner)-> Result<Vec<Executable>> {
        trace!(target: "parser", "start build on_exit");
        let mut exec:Vec<Executable> = Vec::new();

        let on_exit_children = s.get_children();

        for child in on_exit_children {
             let cur_child_value = child.get_value();
            let cur_child_name = cur_child_value.get_name();

            let executable_instance = match cur_child_name {
                 "raise" => Executable::build_execut_raise(child)
                         .context("Failed to build 'raise' executable in onexit")?,
                "assign" => Executable::build_execut_assign(child, interner)
                         .context("Failed to build 'assign' executable in onexit")?,
                "if" => Executable::build_execut_if(child, interner)
                         .context("Failed to build 'if' executable in onexit")?,
                "send" => Executable::build_execut_send(child, interner)
                         .context("Failed to build 'send' executable in onexit")?,
                _key => {
                    anyhow::bail!("Unexpected child element '{}' in <onexit> element", _key);
                }
            };
            exec.push(executable_instance);
        }
        trace!(target: "parser", "end build on_exit");
        Ok(exec)
    }

    pub fn get_id(&self)-> String{
        self.id.clone()
    }
    pub fn get_transitions(&self)-> Vec<Transition>{
        self.transitions.clone()
    }
    pub fn get_on_entry(&self)-> Vec<Executable>{
        self.on_entry.clone()
    }
    pub fn get_on_exit(&self)-> Vec<Executable>{
        self.on_exit.clone()
    }

     pub fn stamp(&self){
        print!("State=State\n");
        print!("id={}\n",self.id);
        for value in self.transitions.clone(){
            value.stamp();
        }
        for value in self.on_entry.clone(){
            print!("State=OnEntry\n");
            match value {
                Executable::Assign { location, expr }   => {
                    println!("State=Assign");
                    println!("location: {}",location);
                    println!("expr={:?}",expr);
                },
                Executable::Raise { event }=> {
                    println!("State=Raise");
                    println!("event: {}",event);
                },
                Executable::Send(send)=>{
                    send.stamp();
                }
                Executable::If(r#if)=>{
                    r#if.stamp();
                }
            }
        }
        for value in self.on_exit.clone(){
            print!("State=OnExit\n");
            match value {
                Executable::Assign { location, expr }   => {
                    println!("State=Assign");
                    println!("location: {}",location);
                    println!("expr={:?}",expr);
                },
                Executable::Raise { event }=> {
                    println!("State=Raise");
                    println!("event: {}",event);
                },
                Executable::Send(send)=>{
                    send.stamp();
                }
                Executable::If(r#if)=>{
                    r#if.stamp();
                }
            }
        }
    }
}