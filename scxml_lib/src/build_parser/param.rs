use anyhow::Result;
use anyhow::Context;
use anyhow::anyhow;

use crate::build_tree::tree;
use crate::build_parser::controll_expression;
use boa_ast::{Expression as BoaExpression, StatementListItem};
use boa_ast::scope::Scope;
use boa_ast::Statement;
use boa_parser;

use log::trace;

#[derive(Debug, Clone)]
pub struct Param {
    pub(crate) name: String,
    pub(crate) omg_type: Option<String>,
    pub(crate) expr: BoaExpression,
}

impl Param {
    pub fn new(
        name_c: String,omg_type_c: Option<String>, expr_c: BoaExpression,
    ) -> Self {
        Param {
            name: name_c,
            omg_type: omg_type_c,
            expr: expr_c
        }
    }

    pub fn build_param(s:tree::Tree, interner: &mut boa_interner::Interner)-> Result<Param> {
        trace!(target: "parser", "start build param");

        let mut name_c: Option<String> = None;
        let mut value_c: Option<String> = None;
        let mut expr_c_str: Option<String> = None;
        let mut type_c: Option<Option<String>> = None;

        let param_attributes = s.get_value().get_attribute_list();
        let mut _has_name = false;

        for atr in param_attributes {
            let cur_atr = atr.get_name();
            match cur_atr {
                "name" => {
                    name_c = Some(atr.get_value().to_string());
                    _has_name = true;
                }
                "value" => {
                    value_c = Some(atr.get_value().to_string());
                }
                "expr" => {
                    expr_c_str = Some(atr.get_value().to_string());
                    if let Some(ref mut expr_str) = expr_c_str {
                         *expr_str = controll_expression(expr_str.clone());
                    }
                }
                "type" => {
                    type_c = Some(Some(atr.get_value().to_string()));
                }
                _key => {}
            }
        }

        let final_name_c = name_c.ok_or_else(|| anyhow!("<param> element is missing required attribute 'name'"))?;

        if value_c.is_some() && expr_c_str.is_some() {
            anyhow::bail!("<param> element cannot have both 'value' and 'expr' attributes");
        }

        if value_c.is_none() && expr_c_str.is_none() {
            anyhow::bail!("<param> element must have either 'value' or 'expr' attribute");
        }

        let parsed_expr: BoaExpression = if let Some(expr_string) = expr_c_str {
             let script_result: std::result::Result<boa_ast::Script, boa_parser::Error> =
                boa_parser::Parser::new(boa_parser::Source::from_bytes(&expr_string))
                .parse_script(&Scope::new_global(), interner);

            let script = script_result
                .map_err(|e| anyhow::anyhow!("Boa parser error in expr for param '{}': {}", final_name_c, e))
                .with_context(|| format!("Failed to parse expr for param '{}'", final_name_c))?;

            let first_statement = script.statements().first()
                 .ok_or_else(|| anyhow!("Expected at least one statement in expr for param '{}'", final_name_c))?;

            if let StatementListItem::Statement(Statement::Expression(expression)) = first_statement.to_owned() {
                 expression
            } else {
                 anyhow::bail!("Expected an expression statement in expr for param '{}'", final_name_c);
            }
        } else {
             anyhow::bail!("<param> element must have 'expr' attribute with valid expression if 'value' is not present or its value cannot be parsed as an expression");
        };


        let final_omg_type_o = type_c.flatten();


        let param = Param::new(final_name_c, final_omg_type_o, parsed_expr);
        trace!(target: "parser", "end build param");
        Ok(param)
    }

    pub fn get_name(&self)-> String{
        self.name.clone()
    }
    pub fn get_omg_type(&self)-> Option<String>{
        self.omg_type.clone()
    }
    pub fn get_expr(&self)-> BoaExpression{
        self.expr.clone()
    }

    pub fn set_omg_type(&mut self,o_t:Option<String>){
        self.omg_type = o_t;
    }

    pub fn stamp(&self){
        print!("State=Param\n");
        print!("name={}\n",self.name);
        match self.omg_type.clone() {
            Some(value) =>println!("type={}", value),
            None =>println!("No Type"),
        }
        println!("expr: {:?}",self.expr);
    }

}