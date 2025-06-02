use anyhow::Result;
use anyhow::Context;
use anyhow::anyhow;

use crate::build_tree::tree;
use crate::build_parser::{Executable, controll_expression};

use boa_ast::StatementListItem;
use boa_ast::{Statement, Expression}; // Removed redundant Expression import
use boa_ast::scope::Scope;
use boa_parser;

use log::{trace};

// Define the ElseIf struct at the module level
#[derive(Debug, Clone)]
pub struct ElseIf {
    pub(crate) cond: boa_ast::Expression,
    pub(crate) effects: Vec<Executable>,
}

impl ElseIf {
     pub fn new(cond_c: boa_ast::Expression, effects_c: Vec<Executable>) -> Self {
         ElseIf { cond: cond_c, effects: effects_c }
     }

     // build_else_if MUST return Result<ElseIf>
     // Executable::build_execut_... MUST return Result<Executable> (already done)
     pub fn build_else_if(s: tree::Tree, interner: &mut boa_interner::Interner) -> Result<ElseIf> {
        trace!(target: "parser", "start build elseif");

        let mut cond_c_str: Option<String> = None;
        let mut effects_c: Vec<Executable> = Vec::new();

        let elseif_attributes = s.get_value().get_attribute_list();
        let mut _has_cond = false;
        for atr in elseif_attributes {
             let cur_atr = atr.get_name();
             match cur_atr {
                 "cond" => {
                     cond_c_str = Some(atr.get_value().to_string());
                      if let Some(ref mut cond_str) = cond_c_str {
                         *cond_str = controll_expression(cond_str.clone());
                      }
                     _has_cond = true;
                 }
                 _key => {}
             }
        }

        let final_cond_str = cond_c_str.ok_or_else(|| anyhow!("<elseif> element is missing required attribute 'cond'"))?;
        if final_cond_str.is_empty() {
             anyhow::bail!("'cond' attribute in <elseif> element cannot be empty");
        }

        let script_result: std::result::Result<boa_ast::Script, boa_parser::Error> =
            boa_parser::Parser::new(boa_parser::Source::from_bytes(&final_cond_str))
            .parse_script(&Scope::new_global(), interner);

        let script = script_result
            .map_err(|e| anyhow::anyhow!("Boa parser error in condition for elseif element: {}", e))
            .with_context(|| format!("Failed to parse condition expression for elseif element: '{}'", final_cond_str))?;

        let first_statement = script.statements().first()
             .ok_or_else(|| anyhow!("Expected at least one statement in condition expression for elseif element: '{}'", final_cond_str))?;

        let cond_expression = if let StatementListItem::Statement(Statement::Expression(expression)) = first_statement.to_owned() {
             expression
        } else {
             anyhow::bail!("Expected an expression statement in condition for elseif element: '{}'", final_cond_str);
        };


        let elseif_children = s.get_children();
        for child in elseif_children {
             let cur_child_value = child.get_value();
             let cur_child_name = cur_child_value.get_name();

             let executable_instance = match cur_child_name {
                "raise" => Executable::build_execut_raise(child).context("Failed to build 'raise' in elseif")?,
                "assign" => Executable::build_execut_assign(child, interner).context("Failed to build 'assign' in elseif")?,
                "if" => Executable::build_execut_if(child, interner).context("Failed to build nested 'if' in elseif")?,
                "send" => Executable::build_execut_send(child, interner).context("Failed to build 'send' in elseif")?,
                _key => {
                     anyhow::bail!("Unexpected child element '{}' in <elseif> element", _key);
                }
            };
            effects_c.push(executable_instance); // Push after ?
        }

        let else_if_instance = ElseIf::new(cond_expression, effects_c);
        trace!(target: "parser", "end build elseif");
        Ok(else_if_instance)
    }

    // Added stamp method for ElseIf
    pub fn stamp(&self) {
        print!("State=Elseif\n");
        println!("cond={:?}", self.cond);
        for value in self.effects.clone() {
             value.stamp(); // Assuming Executable has a stamp method
        }
    }
}


// Define the IfProcessingState enum at the module level
#[derive(Copy, Clone)] // Added Copy and Clone to fix the move error in If::build_if
enum IfProcessingState {
    ThenEffects,
    ElseIfBranches,
    ElseEffects,
}


// build_else_effects helper function MUST be at the module level
fn build_else_effects(s:tree::Tree,interner: &mut boa_interner::Interner)-> Result<Vec<Executable>> {
    trace!(target: "parser", "start build else effects");
     let mut exec:Vec<Executable> = Vec::new();
     let else_children = s.get_children();

     for child in else_children {
          let cur_child_value = child.get_value();
          let cur_child_name = cur_child_value.get_name();

          let executable_instance = match cur_child_name {
             "raise" => Executable::build_execut_raise(child).context("Failed to build 'raise' in else")?,
             "assign" => Executable::build_execut_assign(child, interner).context("Failed to build 'assign' in else")?,
             "if" => Executable::build_execut_if(child, interner).context("Failed to build nested 'if' in else")?,
             "send" => Executable::build_execut_send(child, interner).context("Failed to build 'send' in else")?,
             _key => {
                  anyhow::bail!("Unexpected child element '{}' in <else> element", _key);
             }
         };
         exec.push(executable_instance);
     }
    trace!(target: "parser", "end build else effects");
    Ok(exec)
}


#[derive(Debug, Clone)]
pub struct If {
    pub(crate) cond: boa_ast::Expression,
    pub(crate) then_effects: Vec<Executable>,
    pub(crate) else_if_branches: Vec<ElseIf>,
    pub(crate) else_effects: Option<Vec<Executable>>,
}

impl If {
    pub fn new(
        cond_c: boa_ast::Expression,
        then_effects_c: Vec<Executable>,
        else_if_branches_c: Vec<ElseIf>,
        else_effects_c: Option<Vec<Executable>>,
    ) -> Self {
        If {
            cond: cond_c,
            then_effects: then_effects_c,
            else_if_branches: else_if_branches_c,
            else_effects: else_effects_c,
        }
    }

    pub fn build_if(s: tree::Tree, interner: &mut boa_interner::Interner) -> Result<If> {
        trace!(target: "parser", "start build if");

        let mut cond_c_str: Option<String> = None;

        let if_attributes = s.get_value().get_attribute_list();
        for atr in if_attributes {
            let cur_atr = atr.get_name();
            match cur_atr {
                "cond" => {
                    cond_c_str = Some(atr.get_value().to_string());
                    if let Some(ref mut cond_str) = cond_c_str {
                         *cond_str = controll_expression(cond_str.clone());
                    }
                }
                _key => {}
            }
        }

        let final_cond_str = cond_c_str.ok_or_else(|| anyhow!("<if> element is missing required attribute 'cond'"))?;
        if final_cond_str.is_empty() {
             anyhow::bail!("'cond' attribute in <if> element cannot be empty");
        }


        let script_result: std::result::Result<boa_ast::Script, boa_parser::Error> =
            boa_parser::Parser::new(boa_parser::Source::from_bytes(&final_cond_str))
            .parse_script(&Scope::new_global(), interner);

        let script = script_result
            .map_err(|e| anyhow::anyhow!("Boa parser error in condition for if element: {}", e))
            .with_context(|| format!("Failed to parse condition expression for if element: '{}'", final_cond_str))?;

        let first_statement = script.statements().first()
             .ok_or_else(|| anyhow!("Expected at least one statement in condition expression for if element: '{}'", final_cond_str))?;

        let cond_expression = if let StatementListItem::Statement(Statement::Expression(expression)) = first_statement.to_owned() {
             expression
        } else {
             anyhow::bail!("Expected an expression statement in condition for if element: '{}'", final_cond_str);
        };


        let mut then_effects_c: Vec<Executable> = Vec::new();
        let mut else_if_branches_c: Vec<ElseIf> = Vec::new();
        let mut else_effects_c: Option<Vec<Executable>> = None;

        let if_children = s.get_children();

        let mut processing_state = IfProcessingState::ThenEffects;

        for child in if_children {
            let cur_child_value = child.get_value();
            let cur_child_name = cur_child_value.get_name();

            match (processing_state, cur_child_name) {
                (IfProcessingState::ThenEffects, "raise") |
                (IfProcessingState::ThenEffects, "assign") |
                (IfProcessingState::ThenEffects, "if") |
                (IfProcessingState::ThenEffects, "send") => {
                     let executable_instance = match cur_child_name {
                         "raise" => Executable::build_execut_raise(child).context("Failed to build 'raise' executable in if (then)")?,
                         "assign" => Executable::build_execut_assign(child, interner).context("Failed to build 'assign' executable in if (then)")?,
                         "if" => Executable::build_execut_if(child, interner).context("Failed to build nested 'if' executable in if (then)")?,
                         "send" => Executable::build_execut_send(child, interner).context("Failed to build 'send' executable in if (then)")?,
                         _ => unreachable!(),
                     };
                     then_effects_c.push(executable_instance);
                }
                (IfProcessingState::ThenEffects, "elseif") => {
                    processing_state = IfProcessingState::ElseIfBranches;
                     let else_if_branch = ElseIf::build_else_if(child, interner)
                         .context("Failed to build elseif branch")?;
                    else_if_branches_c.push(else_if_branch);
                }
                 (IfProcessingState::ThenEffects, "else") => {
                     processing_state = IfProcessingState::ElseEffects;
                     if else_effects_c.is_some() {
                        anyhow::bail!("Multiple <else> elements in <if> element");
                     }
                     // Call build_else_effects as an associated function of Self or directly if pub
                     let effects = Self::build_else_effects(child, interner).context("Failed to build else content")?; // Corrected call
                     else_effects_c = Some(effects);
                }

                (IfProcessingState::ElseIfBranches, "elseif") => {
                     let else_if_branch = ElseIf::build_else_if(child, interner)
                         .context("Failed to build elseif branch")?;
                    else_if_branches_c.push(else_if_branch);
                }
                 (IfProcessingState::ElseIfBranches, "else") => {
                     processing_state = IfProcessingState::ElseEffects;
                     if else_effects_c.is_some() {
                        anyhow::bail!("Multiple <else> elements in <if> element");
                     }
                     // Call build_else_effects as an associated function of Self or directly if pub
                     let effects = Self::build_else_effects(child, interner).context("Failed to build else content")?; // Corrected call
                     else_effects_c = Some(effects);
                }

                (IfProcessingState::ElseEffects, _) => {
                     anyhow::bail!("Executable content or other elements must appear after <else> in <if>");
                }

                 (IfProcessingState::ElseIfBranches, _) |
                 (IfProcessingState::ElseEffects, "elseif") |
                 (IfProcessingState::ElseEffects, "else") |
                 (_, _) => {
                     anyhow::bail!("Unexpected child element '{}' in <if> element or invalid element order", cur_child_name);
                 }
            }
        }


        let if_instance = If::new(cond_expression, then_effects_c, else_if_branches_c, else_effects_c);
        trace!(target: "parser", "end build if");
        Ok(if_instance)
    }

    // build_else_effects MUST return Result<Vec<Executable>>
    // Executable::build_execut_... MUST return Result<Executable>
    fn build_else_effects(s:tree::Tree,interner: &mut boa_interner::Interner)-> Result<Vec<Executable>> {
        trace!(target: "parser", "start build else effects");
         let mut exec:Vec<Executable> = Vec::new();
         let else_children = s.get_children();

         for child in else_children {
              let cur_child_value = child.get_value();
              let cur_child_name = cur_child_value.get_name();

              let executable_instance = match cur_child_name {
                 "raise" => Executable::build_execut_raise(child).context("Failed to build 'raise' in else")?,
                 "assign" => Executable::build_execut_assign(child, interner).context("Failed to build 'assign' in else")?,
                 "if" => Executable::build_execut_if(child, interner).context("Failed to build nested 'if' in else")?,
                 "send" => Executable::build_execut_send(child, interner).context("Failed to build 'send' in else")?,
                 _key => {
                      anyhow::bail!("Unexpected child element '{}' in <else> element", _key);
                 }
             };
             exec.push(executable_instance);
         }
        trace!(target: "parser", "end build else effects");
        Ok(exec)
    }

    // Added stamp method for If and ElseIf if needed
    pub fn stamp(&self){ // Implement the stamp method
        print!("State=IF\n");
        println!("cond={:?}",self.cond);

        print!("State=ThenEffects\n");
        for value in self.then_effects.clone(){
            value.stamp(); // Assuming Executable has a stamp method (It needs one!)
        }

        for branch in self.else_if_branches.clone(){
            branch.stamp();
        }

        if let Some(else_execs) = self.else_effects.clone() {
             print!("State=Else\n");
             for value in else_execs {
                 value.stamp();
             }
        }
    }


    pub fn get_cond(&self)-> boa_ast::Expression{
        self.cond.clone()
    }
    pub fn get_then_effects(&self)-> Vec<Executable>{
        self.then_effects.clone()
    }
    pub fn get_else_if_branches(&self)-> Vec<ElseIf>{
        self.else_if_branches.clone()
    }
    pub fn get_else_effects(&self)-> Option<Vec<Executable>>{
        self.else_effects.clone()
    }
}