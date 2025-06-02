pub mod state_conteiner;
pub mod attribute_value;
pub mod state;
pub mod tree;

use anyhow::{Result, Context}; // Removed unused 'anyhow' import variant

use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::fs::File;

use lrlex::lrlex_mod;
use lrpar::lrpar_mod;

lrlex_mod!("calc.l");
lrpar_mod!("calc.y");

use state::State;
use attribute_value::AttributeValue;
use tree::Tree;

fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where P: AsRef<Path>, {
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

pub fn build_tree(s: PathBuf) -> Result<Tree> {
    let mut is_start = true;
    let attributes_start_state: Vec<AttributeValue> = vec![];
    let start_state = State::new("Start".to_string(), attributes_start_state);
    let children: Vec<Tree> = vec![];
    let mut root = Tree::new(0, -1, start_state, children);

    let lexerdef = calc_l::lexerdef();

    let path = s;

    let extension = path.extension().and_then(|ext| ext.to_str());
    if !extension.map_or(false, |ext| ext == "scxml") {
         anyhow::bail!("File must have scxml extension, but found: {}", path.display());
    }

    let lines = read_lines(&path).context(format!("Failed to read file: {}", path.display()))?;

    let mut count = 0;
    let mut cur_node: i32 = 0;

    for line in lines {
         let line = line.context("Failed to read line from file")?;

         let lt = line.trim();
         if !lt.is_empty() {
             let lexer = lexerdef.lexer(lt);
             let (res, errs) = calc_y::parse(&lexer);

             if !errs.is_empty() {
                  let first_err = errs[0].pp(&lexer, &calc_y::token_epp);
                  anyhow::bail!("Parsing error in {}: {}", path.display(), first_err);
             }

             match res {
                 Some(Ok(r)) => {
                     if is_start {
                         is_start = false;
                         root.set_value(r.get_value());
                         count += 1;
                     } else {
                         let r_value = r.get_value();
                         let node_value_name = r_value.get_name();

                         let current_node_value = root.get_value_specific_node(cur_node);
                         let current_node_name = current_node_value.get_name();

                         let cond_value = r.get_cond();

                         if node_value_name == "elseif" {
                             if current_node_name == "if" || current_node_name == "elseif" {
                                 if current_node_name == "elseif" {
                                     cur_node = root.get_father_id(cur_node);
                                 }
                                 cur_node = root.insert_node(r_value, cur_node, count, false);
                             } else {
                                 anyhow::bail!("ERROR: elseif needs to be a child of if or elseif in {}", path.display());
                             }
                         } else if node_value_name == "else" {
                              if current_node_name == "if" || current_node_name == "elseif" {
                                 if current_node_name == "elseif" {
                                     cur_node = root.get_father_id(cur_node);
                                 }
                                 cur_node = root.insert_node(r_value, cur_node, count, false);
                             } else {
                                 anyhow::bail!("ERROR: else needs to be a child of if or elseif in {}", path.display());
                             }
                         } else if node_value_name == "CLOSE" {
                              if current_node_name == "if" || current_node_name == "elseif" || current_node_name == "else" {
                                   if current_node_name == "elseif" || current_node_name == "else" {
                                     cur_node = root.get_father_id(cur_node);
                                   }
                                   cur_node = root.insert_node(r_value, cur_node, count, cond_value);
                              } else {
                                   cur_node = root.insert_node(r_value, cur_node, count, cond_value);
                              }
                         } else {
                              cur_node = root.insert_node(r_value, cur_node, count, cond_value);
                         }
                         count += 1;
                     }
                 },
                 Some(Err(e)) => {
                     anyhow::bail!("Semantic error during parsing in {}: {:?}", path.display(), e);
                 }
                 None => {
                      anyhow::bail!("Parser returned no result for line in {}", path.display());
                 }
             }
         }
    }

    Ok(root)
}