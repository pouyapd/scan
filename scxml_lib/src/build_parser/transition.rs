use anyhow::Result;
use anyhow::Context;
use anyhow::anyhow;

use crate::build_tree::tree;
use crate::build_parser::{Executable, controll_expression};

use boa_ast::StatementListItem;
use boa_ast::{Statement, Expression};
use boa_ast::scope::Scope;
use boa_parser;

use log::{trace};

#[derive(Debug, Clone)]
pub struct Transition {
    pub(crate) event: Option<String>,
    pub(crate) target: String,
    pub(crate) cond: Option<boa_ast::Expression>,
    pub(crate) effects: Vec<Executable>,
}

impl Transition {
    pub fn new(
        event_c:Option<String>,target_c:String,cond_c:Option<boa_ast::Expression>,effects_c:Vec<Executable>) -> Self{
        Transition {
            event: event_c,
            target: target_c,
            cond: cond_c,
            effects: effects_c
        }
    }

    pub fn build_transition(s:tree::Tree,interner: &mut boa_interner::Interner)-> Result<Transition> {
        trace!(target: "parser", "start build transition");

        let mut event_c: Option<String> = None;
        let mut target_c_str: Option<String> = None; // Keep as Option<String>
        let mut cond_c_str: Option<String> = None; // *** Changed to Option<String> ***


        let transition_attributes = s.get_value().get_attribute_list();

        let mut _has_target = false;

        for atr in transition_attributes {
            let cur_atr = atr.get_name();
            match cur_atr {
                "event" => {
                    event_c = Some(atr.get_value().to_string());
                }
                "target" =>{
                    target_c_str = Some(atr.get_value().to_string()); // Assigned Option<String>
                    _has_target = true;
                }
                "cond" =>{
                    let value_str = atr.get_value().to_string();
                    let controlled_value = controll_expression(value_str);
                    cond_c_str = Some(controlled_value); // *** Assigned Option<String> ***
                }
                _key => {}
            }
        }

        let final_target_c = target_c_str.ok_or_else(|| anyhow!("<transition> element is missing required attribute 'target'"))?;
        let final_event_c = event_c;

        let cond_expression: Option<Expression> = if let Some(cond_string) = cond_c_str {
            if cond_string.is_empty() {
                None
            } else {
                let script_result: std::result::Result<boa_ast::Script, boa_parser::Error> =
                    boa_parser::Parser::new(boa_parser::Source::from_bytes(&cond_string))
                    .parse_script(&Scope::new_global(), interner);

                let script = script_result
                    .map_err(|e| anyhow::anyhow!("Boa parser error in condition for target '{}': {}", final_target_c, e))
                    .with_context(|| format!("Failed to parse condition expression for transition to '{}'", final_target_c))?;

                let first_statement = script.statements().first()
                     .ok_or_else(|| anyhow!("Expected at least one statement in condition expression for transition to '{}'", final_target_c))?;

                if let StatementListItem::Statement(Statement::Expression(expression)) = first_statement.to_owned() {
                     Some(expression)
                } else {
                     anyhow::bail!("Expected an expression statement in condition for transition to '{}'", final_target_c);
                }
            }
        } else {
            None
        };

        let mut final_effect_c:Vec<Executable> = Vec::new();
        let transition_children = s.get_children();

        for child in transition_children {
            let cur_child_value = child.get_value();
            let cur_child_name = cur_child_value.get_name();

            let executable_instance = match cur_child_name {
                "raise" => Executable::build_execut_raise(child)
                    .context("Failed to build 'raise' executable in transition")?,
                "assign" => Executable::build_execut_assign(child, interner)
                    .context("Failed to build 'assign' executable in transition")?,
                "if" => Executable::build_execut_if(child, interner)
                    .context("Failed to build 'if' executable in transition")?,
                "send" => Executable::build_execut_send(child, interner)
                    .context("Failed to build 'send' executable in transition")?,
                _key => {
                     anyhow::bail!("Unexpected child element '{}' in <transition> element", _key);
                }
            };
            final_effect_c.push(executable_instance);
        }

        let transition = Transition::new(final_event_c,final_target_c,cond_expression,final_effect_c);
        trace!(target: "parser", "end build transition");
        Ok(transition)
    }

    pub fn get_event(&self)-> Option<String>{
        self.event.clone()
    }
    pub fn get_target(&self)-> String{
        self.target.clone()
    }
    pub fn get_cond(&self)-> Option<boa_ast::Expression>{
        self.cond.clone()
    }
    pub fn get_effects(&self)-> Vec<Executable>{
        self.effects.clone()
    }

     pub fn stamp(&self){
        print!("Transition\n");
        match self.event.clone() {
            Some(value) =>println!("event={}", value),
            None =>println!("The `no event."),
        }
        print!("target={}\n",self.target);
        match self.cond.clone(){
            Some(value) => println!("cond={:?}",value),
            None =>println!("The `no cond."),
        }
        for value in self.effects.clone(){
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