use anyhow::Result;
use anyhow::Context;
use anyhow::anyhow;

use crate::build_tree::tree;
use crate::build_parser::{Send, If, controll_expression};

use boa_ast::StatementListItem;
use boa_ast::{Statement, Expression};
use boa_ast::scope::Scope;
use boa_parser;

use log::{trace};


#[derive(Debug, Clone)]
pub enum Executable {
    Assign {
        location: String,
        expr: boa_ast::Expression,
    },
    Raise {
        event: String,
    },
    Send(Send),
    If(If),
}

impl Executable {
    pub fn new_raise(event_c:String) -> Self {
        Executable::Raise { event: event_c }
    }

    pub fn new_assign(
        location_c:String,expr_c: boa_ast::Expression
    ) -> Self {
        Executable::Assign {
            location: location_c,
            expr: expr_c
        }
    }

    pub fn new_send(send_c:Send)->Self{
        Executable::Send(send_c)
    }

    pub fn new_if(if_c:If)->Self{
        Executable::If(if_c)
    }

    // Add the stamp method for the Executable enum
    pub fn stamp(&self) {
        match self {
            Executable::Assign { location, expr }   => {
                println!("State=Assign");
                println!("location: {}", location);
                println!("expr={:?}", expr);
            },
            Executable::Raise { event }=> {
                println!("State=Raise");
                println!("event: {}", event);
            },
            Executable::Send(send)=>{
                send.stamp();
            },
            Executable::If(r#if)=>{
                r#if.stamp();
            }
        }
    }


    pub fn build_execut_raise(s:tree::Tree) -> Result<Executable> {
        trace!(target: "parser", "start build raise");
        let mut event_c: Option<String> = None;

        let raise_attributes = s.get_value().get_attribute_list();

        for atr in raise_attributes {
            let cur_atr = atr.get_name();
            match cur_atr {
                "event" => {
                    event_c = Some(atr.get_value().to_string());
                }
                _key => {}
            }
        }

        let final_event_c = event_c.ok_or_else(|| anyhow!("<raise> element is missing required attribute 'event'"))?;

        let execute = Executable::new_raise(final_event_c);
        trace!(target: "parser", "end build raise");
        Ok(execute)
    }

    pub fn build_execut_assign(s:tree::Tree,interner: &mut boa_interner::Interner)-> Result<Executable> {
        trace!(target: "parser", "start build assign");
        let mut location_c: Option<String> = None;
        let mut expr_c_str: Option<String> = None; // *** Changed to Option<String> ***


        let assign_attributes = s.get_value().get_attribute_list();

        let mut _has_location = false;
        let mut _has_expr = false;

        for atr in assign_attributes {
            let cur_atr = atr.get_name();
            match cur_atr {
                "location" => {
                    location_c = Some(atr.get_value().to_string());
                    _has_location = true;
                }
                "expr" => {
                    expr_c_str = Some(atr.get_value().to_string()); // Assigned Option<String>
                    if let Some(ref mut expr_str) = expr_c_str {
                         *expr_str = controll_expression(expr_str.clone());
                     }
                    _has_expr = true;
                }
                _key => {}
            }
        }

        let final_location_c = location_c.ok_or_else(|| anyhow!("<assign> element is missing required attribute 'location'"))?;
        let final_expr_c_str = expr_c_str; // Now Option<String> matches

        let expression: Option<Expression> = if let Some(expression_string) = final_expr_c_str {
             let script_result: std::result::Result<boa_ast::Script, boa_parser::Error> =
                boa_parser::Parser::new(boa_parser::Source::from_bytes(&expression_string))
                .parse_script(&Scope::new_global(), interner);

            let script = script_result
                .map_err(|e| anyhow::anyhow!("Boa parser error in expression for assign element '{}': {}", final_location_c, e))
                .with_context(|| format!("Failed to parse expression for assign element '{}'", final_location_c))?;

            let first_statement = script.statements().first()
                 .ok_or_else(|| anyhow!("Expected at least one statement in expression for assign element '{}'", final_location_c))?;

            if let StatementListItem::Statement(Statement::Expression(expression)) = first_statement.to_owned() {
                 Some(expression)
            } else {
                 anyhow::bail!("Expected an expression statement in expression for assign element '{}'", final_location_c);
            }
        } else {
             anyhow::bail!("<assign> element is missing required attribute 'expr' or its expression is invalid");
        };

        let final_expr_o = expression.ok_or_else(|| anyhow!("<assign> element is missing required attribute 'expr' or its expression is invalid"))?;

        let execute = Executable::new_assign(final_location_c, final_expr_o);
        trace!(target: "parser", "end build assign");
        Ok(execute)
    }

    pub fn build_execut_send(s:tree::Tree,interner: &mut boa_interner::Interner)-> Result<Executable> {
        let send_c = Send::build_send(s, interner)
             .context("Failed to build send executable")?;

        let execute = Executable::new_send(send_c);
        trace!(target: "parser", "end build send");
        Ok(execute)
    }

    pub fn build_execut_if(s:tree::Tree,interner: &mut boa_interner::Interner)-> Result<Executable> {
        let if_c = If::build_if(s, interner)
            .context("Failed to build if executable")?;

        let execute = Executable::new_if(if_c);
        trace!(target: "parser", "end build if");
        Ok(execute)
    }
}