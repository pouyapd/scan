use anyhow::Result;
use anyhow::Context;
use anyhow::anyhow;

use crate::build_tree::tree;
use crate::build_parser::controll_expression;
use boa_ast::StatementListItem;
use boa_ast::{Statement, Expression};
use boa_ast::scope::Scope;
use boa_parser;
use log::{trace};

#[derive(Debug, Clone)]
pub struct Data {
    pub(crate) id: String,
    pub(crate) expression: Option<boa_ast::Expression>,
    pub(crate) omg_type: String,
}

impl Data {
    fn new(
        id_c:String,expression_c:Option<boa_ast::Expression>,omg_type_c:String
    ) -> Self {
        Data {
            id: id_c,
            expression: expression_c,
            omg_type: omg_type_c
        }
    }

    pub fn build_data(s:tree::Tree,
        interner: &mut boa_interner::Interner
    ) -> Result<Data> {
        trace!(target: "parser", "start build data");

        let mut id_c = "".to_string();
        let mut omg_type_c= "".to_string();
        let mut expr_c= "".to_string();

        let data_attributes = s.get_value().get_attribute_list();

        // Removed unused has_id, has_expr, has_type variables
        // let mut _has_id = false;
        // let mut _has_expr = false;
        // let mut _has_type = false;

        for atr in data_attributes {
            let cur_atr = atr.get_name();
            match cur_atr {
                "id" => {
                    id_c = atr.get_value().to_string();
                    // _has_id = true;
                }
                "expr" =>{
                    expr_c = atr.get_value().to_string();
                    expr_c = controll_expression(expr_c);
                    // _has_expr = true;
                }
                "type" =>{
                    omg_type_c = atr.get_value().to_string();
                    // _has_type = true;
                }
                _key => {}
            }
        }

        if id_c.is_empty() {
           anyhow::bail!("<data> element is missing required attribute 'id'");
        }

        let expr_o = if expr_c.is_empty() || expr_c == "none" {
            None
        } else {
            Some(expr_c)
        };

        let expression: Option<Expression> = if let Some(expression_string) = expr_o {
            let script_result: std::result::Result<boa_ast::Script, boa_parser::Error> =
                boa_parser::Parser::new(boa_parser::Source::from_bytes(&expression_string))
                .parse_script(&Scope::new_global(), interner);

            let script = script_result
                .map_err(|e| anyhow::anyhow!("Boa parser error: {}", e))
                .with_context(|| format!("Failed to parse JavaScript expression for data element '{}'", id_c))?;

            let first_statement = script.statements().first()
                 .ok_or_else(|| anyhow!("Expected at least one statement in data expression for '{}'", id_c))?;

            if let StatementListItem::Statement(Statement::Expression(expression)) = first_statement.to_owned() {
                 Some(expression)
            } else {
                 anyhow::bail!("Expected an expression statement in data expression for '{}'", id_c);
            }
        } else {
            None
        };

        let data = Data::new(id_c, expression, omg_type_c);

        trace!(target: "parser", "end build data");
        Ok(data)
    }

    pub fn get_id(&self)-> String{
        self.id.clone()
    }
    pub fn get_expression(&self)-> Option<boa_ast::Expression>{
        self.expression.clone()
    }
    pub fn get_omg_type(&self)-> String{
        self.omg_type.clone()
    }

    pub fn stamp(&self){
        print!("State=Data\n");
        print!("id={}\n",self.id);
        match self.expression.clone() {
            Some(value) =>println!("expression={:?}", value),
            None =>println!("No Expression"),
        }
        println!("type={}",self.omg_type);
    }
}