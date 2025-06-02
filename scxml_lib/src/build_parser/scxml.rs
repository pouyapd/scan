/*Scxml class */
use crate::build_tree::tree;
use crate::build_parser::{State, Data};
use std::collections::HashMap;

use log::{info, trace};

use anyhow::{Result, Context}; // Removed unused 'anyhow' import
use anyhow::anyhow;

#[derive(Debug, Clone)]
pub struct Scxml {
    pub(crate) id: String,
    pub(crate) initial: String,
    pub(crate) datamodel: Vec<Data>,
    pub(crate) states: HashMap<String, State>,
}

impl Scxml {
    pub fn new(id_c: String, initial_c: String, datamodel_c: Vec<Data>, states_c: HashMap<String, State>) -> Self {
        Scxml {
            id: id_c,
            initial: initial_c,
            datamodel: datamodel_c,
            states: states_c,
        }
    }

    pub fn build_scxml(
        s: tree::Tree,
        interner: &mut boa_interner::Interner
    ) -> Result<Self> {
        info!(target: "parser", "parsing fsm");
        trace!(target: "parser", "start build scxml");

        let mut id_c = "".to_string();
        let mut initial_c = "".to_string();
        let mut states_c: HashMap<String, State> = HashMap::new(); // datamodel_c and state_c don't need mut initially if assigned result of function call later

        let scxml_attributes = s.get_value().get_attribute_list();

        let mut _has_name = false;
        let mut _has_initial = false;

        for atr in scxml_attributes {
            let cur_atr_name = atr.get_name();
            match cur_atr_name {
                "name" => {
                    id_c = atr.get_value().to_string();
                    _has_name = true;
                }
                "initial" => {
                    initial_c = atr.get_value().to_string();
                    _has_initial = true;
                }
                _key => {}
            }
        }

         if id_c.is_empty() {
            anyhow::bail!("Missing required attribute 'name' in <scxml> element");
        }
        if initial_c.is_empty() {
            anyhow::bail!("Missing required attribute 'initial' in <scxml> element");
        }


        let scxml_children = s.get_children();

        // Collect datamodel and states separately before creating Scxml
        let mut collected_datamodel: Option<Vec<Data>> = None; // Use Option to ensure only one datamodel
        let mut collected_states: HashMap<String, State> = HashMap::new();


        for child in scxml_children {
            let cur_child_value = child.get_value();
            let cur_child_name = cur_child_value.get_name();

            match cur_child_name {
                "state" => {
                    let mut id_s = "".to_string();
                    let state_attributes = child.get_value().get_attribute_list();

                    let mut _has_state_id = false;
                    for atr in state_attributes {
                        let cur_atr_name = atr.get_name();
                        match cur_atr_name {
                            "id" => {
                                id_s = atr.get_value().to_string();
                                _has_state_id = true;
                            }
                            _key => {}
                        }
                    }

                    if id_s.is_empty() {
                        anyhow::bail!("Missing required attribute 'id' in <state> element");
                    }

                    let state_instance = State::build_state(child, interner)
                        .context(format!("Failed to build state with id '{}'", id_s))?;

                    collected_states.insert(id_s, state_instance); // Insert into the map
                }
                "datamodel" => {
                    if collected_datamodel.is_some() { // Check for multiple datamodels
                         anyhow::bail!("Only one <datamodel> element is allowed in <scxml>");
                    }
                    let datamodel_c = Self::build_datamodel(child, interner)
                        .context("Failed to build datamodel")?;
                    collected_datamodel = Some(datamodel_c); // Store the built datamodel
                }
                _key => {}
            }
        }

        // Ensure datamodel was present (if required by spec)
        let final_datamodel_c = collected_datamodel.ok_or_else(|| anyhow!("Missing required <datamodel> element in <scxml>"))?;


        let scxml = Scxml::new(id_c, initial_c, final_datamodel_c, collected_states); // Use the collected values

        trace!(target: "parser", "end build scxml");
        Ok(scxml)
    }

    pub fn build_datamodel(s: tree::Tree, interner: &mut boa_interner::Interner) -> Result<Vec<Data>> {
        trace!(target: "parser", "start build datamodel");
        let mut vec_data_c: Vec<Data> = Vec::new();

        let datamodel_children = s.get_children();

        for child in datamodel_children {
            let cur_child_value = child.get_value();
            let cur_child_name = cur_child_value.get_name();

            match cur_child_name {
                "data" => {
                    let data_instance = Data::build_data(child, interner)
                        .context("Failed to build data element")?;
                    vec_data_c.push(data_instance);
                }
                _key => {}
            }
        }
        trace!(target: "parser", "end build datamodel");
        Ok(vec_data_c)
    }

    pub fn get_id(&self) -> String {
        self.id.clone()
    }
    pub fn get_initial(&self) -> String {
        self.initial.clone()
    }
    pub fn get_datamodel(&self) -> Vec<Data> {
        self.datamodel.clone()
    }
    pub fn get_states(&self) -> HashMap<String, State> {
        self.states.clone()
    }

    pub fn stamp(&self) {
        print!("State=Scxml\n");
        print!("id={}\n", self.id);
        print!("initial={}\n", self.initial);
        for data in self.datamodel.clone() {
            data.stamp();
        }
        for (_key, value) in self.states.clone() {
            value.stamp();
        }
    }
}