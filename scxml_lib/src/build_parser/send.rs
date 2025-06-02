use anyhow::Result;
use anyhow::Context;
use anyhow::anyhow;

use crate::build_tree::tree;
use crate::build_parser::param::Param;
use crate::build_parser::controll_expression;
use boa_ast::{Expression as BoaExpression, StatementListItem}; // Kept BoaExpression and StatementListItem as they are used
use boa_ast::scope::Scope;
use boa_ast::Statement; // Added Statement import
use boa_parser;

use log::trace;

#[derive(Debug, Clone)]
pub enum Target {
    Id(String),
    Expr(boa_ast::Expression),
}

#[derive(Debug, Clone)]
pub struct Send {
    pub(crate) event: String,
    pub(crate) target: Option<Target>,
    pub(crate) delay: Option<u32>,
    pub(crate) params: Vec<Param>,
}

impl Send {
    pub fn new(
        event_c: String,target_c: Option<Target>,delay_c: Option<u32>,params_c: Vec<Param>
    ) -> Self {
        Send {
            event: event_c,
            target: target_c,
            delay: delay_c,
            params: params_c
        }
    }

    pub fn build_send(s:tree::Tree,interner: &mut boa_interner::Interner)-> Result<Send> {
        trace!(target: "parser", "start build send");

        let mut event_c_str: Option<String> = None;
        let mut target_c_str: Option<String> = None;
        let mut targetexpr_c_str: Option<String> = None;
        let mut delay_c_str: Option<String> = None;

        let send_attributes = s.get_value().get_attribute_list();

        for atr in send_attributes {
            let cur_atr = atr.get_name();
            match cur_atr {
                "event" => {
                    event_c_str = Some(atr.get_value().to_string());
                }
                "target" => {
                    target_c_str = Some(atr.get_value().to_string());
                }
                "targetexpr" => {
                    targetexpr_c_str = Some(atr.get_value().to_string());
                    if let Some(ref mut targetexpr_str) = targetexpr_c_str {
                         *targetexpr_str = controll_expression(targetexpr_str.clone());
                     }
                }
                "delay" => {
                    delay_c_str = Some(atr.get_value().to_string());
                }
                _key => {}
            }
        }

        let final_event_c = event_c_str.ok_or_else(|| anyhow!("<send> element is missing required attribute 'event'"))?;

        if target_c_str.is_some() && targetexpr_c_str.is_some() {
            anyhow::bail!("<send> element cannot have both 'target' and 'targetexpr' attributes");
        }

        let final_target: Option<Target> = if let Some(target_id) = target_c_str {
            Some(Target::Id(target_id))
        } else if let Some(targetexpr_string) = targetexpr_c_str {
             let script_result: std::result::Result<boa_ast::Script, boa_parser::Error> =
                boa_parser::Parser::new(boa_parser::Source::from_bytes(&targetexpr_string))
                .parse_script(&Scope::new_global(), interner);

            let script = script_result
                .map_err(|e| anyhow::anyhow!("Boa parser error in targetexpr for send element: {}", e))
                .with_context(|| format!("Failed to parse targetexpr for send element: '{}'", targetexpr_string))?;

            let first_statement = script.statements().first()
                 .ok_or_else(|| anyhow!("Expected at least one statement in targetexpr for send element: '{}'", targetexpr_string))?;

            if let StatementListItem::Statement(Statement::Expression(expression)) = first_statement.to_owned() {
                 Some(Target::Expr(expression))
            } else {
                 anyhow::bail!("Expected an expression statement in targetexpr for send element: '{}'", targetexpr_string);
            }
        } else {
            None
        };

        let final_delay_cc: Option<u32> = if let Some(delay_str) = delay_c_str {
             delay_str.parse::<u32>()
                  .map_err(|e| anyhow::anyhow!("Failed to parse delay attribute '{}' as u32: {}", delay_str, e))
                  .context(format!("Invalid delay attribute in <send> element: '{}'", delay_str))?
                  .into()
        } else {
            None
        };


        let mut params_c: Vec<Param> = Vec::new();
        let send_children = s.get_children();

        for child in send_children {
            let cur_child_value = child.get_value();
            let cur_child_name = cur_child_value.get_name();

            match cur_child_name {
                "param" => {
                    let param_instance = Param::build_param(child, interner)
                        .context("Failed to build param in send element")?;
                    params_c.push(param_instance);
                }
                _key => {
                     anyhow::bail!("Unexpected child element '{}' in <send> element", _key);
                }
            }
        }

        let send = Send::new(final_event_c, final_target, final_delay_cc, params_c);
        trace!(target: "parser", "end build send");
        Ok(send)
    }

    pub fn get_event(&self)-> String{
        self.event.clone()
    }
    pub fn get_target(&self)-> Option<Target>{
        self.target.clone()
    }
    pub fn get_delay(&self)-> Option<u32>{
        self.delay.clone()
    }
    pub fn get_params(&self)-> Vec<Param>{
        self.params.clone()
    }

     pub fn stamp(&self){
        print!("State=Send\n");
        print!("event={}\n",self.event);
        if let Some(value) = self.target.clone(){
            match value {
                Target::Id(id)=>{
                    println!("target (id): {}",id);
                },
                Target::Expr(expr)=>{
                    println!("target (expr): {:?}",expr);
                }
            }
        }
        match self.delay.clone() {
            Some(value) =>println!("delay={}", value),
            None =>println!("No Delay"),
        }
        for value in self.params.clone(){
            value.stamp();
        }
    }
}