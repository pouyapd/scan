use super::Model;
use crate::parser::{
    self, Automaton, BoolOp, ConstantDeclaration, Edge, Expression, Guard, Location, NumCompOp,
    PropertyExpression, Sync, VariableDeclaration,
};
use anyhow::{Context, anyhow, bail};
use either::Either;
use rand::{SeedableRng, rngs::SmallRng};
use scan_core::{
    Mtl, MtlOracle, PgModel, Type, TypeError, Val,
    program_graph::{self, Action, PgExpression, ProgramGraphBuilder, Var},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Not,
};

#[derive(Clone)]
pub struct JaniModelData {
    pub actions: HashMap<Action, String>,
    pub ports: Vec<(String, Type)>,
    pub guarantees: Vec<String>,
}

pub(crate) fn build(jani_model: Model) -> anyhow::Result<(PgModel, MtlOracle, JaniModelData)> {
    let builder = JaniBuilder::default();
    builder.build(jani_model)
}

#[derive(Default)]
struct JaniBuilder {
    system_actions: HashMap<String, program_graph::Action>,
    global_vars: BTreeMap<String, (Var, Type)>,
    global_constants: HashMap<String, Val>,
}

impl JaniBuilder {
    const RNG: &str = "__RNG__";
    const GEN: &str = "__GEN__";

    pub(crate) fn build(
        mut self,
        mut jani_model: Model,
    ) -> anyhow::Result<(PgModel, MtlOracle, JaniModelData)> {
        // WARN Necessary "normalization" process
        self.normalize(&mut jani_model);

        let mut pgb = ProgramGraphBuilder::new();

        jani_model.system.syncs.iter().for_each(|sync| {
            let result = sync.result.as_ref().expect("no silent actions");
            if !self.system_actions.contains_key(result) {
                let action = pgb.new_action();
                let prev = self.system_actions.insert(result.clone(), action);
                assert!(prev.is_none(), "checked by above if condition");
            }
        });

        jani_model
            .constants
            .iter()
            .try_for_each(|c| self.add_global_constant(c))?;
        jani_model
            .variables
            .iter()
            .try_for_each(|var| self.add_global_var(&mut pgb, var))?;

        let init = pgb.new_action();
        jani_model
            .system
            .elements
            .iter()
            .enumerate()
            .try_for_each(|(e_idx, element)| {
                let automaton = jani_model
                    .automata
                    .iter()
                    .find(|a| a.name == element.automaton)
                    .ok_or(anyhow!(
                        "element '{}' is not a known automaton",
                        element.automaton
                    ))?;
                self.build_automaton(&jani_model, &mut pgb, automaton, init, e_idx)
                    .with_context(|| format!("failed to build automaton '{}'", element.automaton))
            })?;

        // Add properties
        let properties = jani_model
            .properties
            .iter()
            .map(|p| {
                self.build_property(&p.expression)
                    .map(|p| p.right_or_else(Mtl::Atom))
            })
            .collect::<Result<Vec<_>, _>>()?;
        fn extract_predicates(prop: &Mtl<PgExpression>) -> Vec<PgExpression> {
            match prop {
                Mtl::Atom(pred) => vec![pred.clone()],
                Mtl::Until(lhs, rhs) => vec![lhs.clone(), rhs.clone()],
            }
        }
        fn extract_mtl(prop: &Mtl<PgExpression>, idx: &mut usize) -> Mtl<usize> {
            match prop {
                Mtl::Atom(_) => {
                    let prop = Mtl::Atom(*idx);
                    *idx += 1;
                    prop
                }
                Mtl::Until(_, _) => {
                    let prop = Mtl::Until(*idx, *idx + 1);
                    *idx += 2;
                    prop
                }
            }
        }
        let mut idx = 0;
        let mut oracle = MtlOracle::default();
        properties
            .iter()
            .map(|p| extract_mtl(p, &mut idx))
            .for_each(|mtl| oracle.add_guarantee(mtl));
        let predicates = properties
            .into_iter()
            .flat_map(|prop| extract_predicates(&prop).into_iter())
            .collect::<Vec<_>>();
        let global_vars = self
            .global_vars
            .values()
            .map(|(var, _)| var)
            .copied()
            .collect();

        // Finalize, build and return everything
        let pg = pgb.build();
        let pg_model = PgModel::new(pg, SmallRng::from_os_rng(), global_vars, predicates);
        let data = self.data(jani_model);

        Ok((pg_model, oracle, data))
    }

    // An action in JANI doesn not carry effects,
    // so we need to duplicate actions until each one has unique effects.
    // The modified model is such that:
    //
    // - Every action has a unique set of assignments (duplicates actions).
    // - Every edge has a unique destination (because destinations are tied to assignments).
    // - Syncs are updated with new actions
    //   (if action a is duplicated to a', all syncs containing a are duplicated with a' and a new result
    //   so that result corresponds to a unique set of assignments, union of all the assignments of each of its actions).
    // - Probability is encoded in guard
    fn normalize(&mut self, jani_model: &mut Model) {
        // index is global so there is no risk of name-clash
        let mut idx = 0;
        let rng = Expression::Identifier(String::from(Self::RNG));
        for automaton in &mut jani_model.automata {
            let mut new_edges = Vec::new();
            for edge in &mut automaton.edges {
                // Avoid silent actions
                if edge.action.is_none() {
                    edge.action = Some(Self::GEN.to_string() + &idx.to_string());
                    idx += 1;
                }
                let edge_action = edge.action.clone().expect("no silent action");
                assert!(!edge_action.is_empty());
                let mut prob: Option<Expression> = None;
                for dest in &mut edge.destinations {
                    // Add probability to guard
                    let mut guard_exp = edge.guard.as_ref().map(|guard| guard.exp.clone());
                    if let Some(ref p) = dest.probability {
                        // First probability case has no lower bound
                        if let Some(ref prob) = prob {
                            let lower_bound = Expression::NumComp {
                                op: NumCompOp::Leq,
                                left: Box::new(prob.clone()),
                                right: Box::new(rng.clone()),
                            };
                            guard_exp = guard_exp
                                .map(|g| Expression::Bool {
                                    op: BoolOp::And,
                                    left: Box::new(lower_bound.clone()),
                                    right: Box::new(g),
                                })
                                .or(Some(lower_bound));
                        }
                        let upper_prob = prob.map_or_else(
                            || p.exp.clone(),
                            |prob| Expression::IntOp {
                                op: parser::IntOp::Plus,
                                left: Box::new(prob),
                                right: Box::new(p.exp.clone()),
                            },
                        );
                        let upper_bound = Expression::NumComp {
                            op: NumCompOp::Less,
                            left: Box::new(rng.clone()),
                            right: Box::new(upper_prob.clone()),
                        };
                        guard_exp = guard_exp
                            .map(|g| Expression::Bool {
                                op: BoolOp::And,
                                left: Box::new(upper_bound.clone()),
                                right: Box::new(g),
                            })
                            .or(Some(upper_bound));
                        // Update accumulated probability
                        prob = Some(upper_prob);
                    } else if let Some(ref prob) = prob {
                        // Last probability could be left implicit
                        let lower_bound = Expression::NumComp {
                            op: NumCompOp::Leq,
                            left: Box::new(prob.clone()),
                            right: Box::new(rng.clone()),
                        };
                        guard_exp = guard_exp
                            .map(|g| Expression::Bool {
                                op: BoolOp::And,
                                left: Box::new(lower_bound.clone()),
                                right: Box::new(g),
                            })
                            .or(Some(lower_bound));
                        // Need to remember this had a probability
                        dest.probability = Some(parser::Probability {
                            exp: Expression::IntOp {
                                op: parser::IntOp::Minus,
                                left: Box::new(Expression::ConstantValue(
                                    parser::ConstantValue::NumberReal(1.),
                                )),
                                right: Box::new(prob.clone()),
                            },
                            comment: String::new(),
                        });
                    }
                    let action;
                    if dest.assignments.is_empty() {
                        // If there are no assignments we don't need a new name.
                        action = edge_action.clone();
                    } else {
                        action = edge_action.clone() + Self::GEN + &idx.to_string();
                        idx += 1;
                    }

                    new_edges.push(Edge {
                        location: edge.location.clone(),
                        action: Some(action.clone()),
                        guard: guard_exp.map(|exp| Guard {
                            exp,
                            comment: String::new(),
                        }),
                        destinations: vec![dest.clone()],
                        comment: String::new(),
                    });

                    // Update syncs with new action (has to synchronise like original one)
                    for e_idx in 0..jani_model.system.elements.len() {
                        if jani_model.system.elements[e_idx].automaton == automaton.name {
                            // Only add new syncs if new action was generated
                            if action != edge_action {
                                let to_add = jani_model
                                    .system
                                    .syncs
                                    .iter()
                                    .filter(|sync| {
                                        sync.synchronise[e_idx]
                                            .as_ref()
                                            .is_some_and(|a| *a == edge_action)
                                    })
                                    .map(|sync| {
                                        let mut synchronise = sync.synchronise.clone();
                                        let _ = synchronise[e_idx].insert(action.clone());
                                        // Generate new unique result action
                                        let result = sync.result.clone().unwrap_or_default()
                                            + Self::GEN
                                            + &idx.to_string();
                                        idx += 1;
                                        Sync {
                                            synchronise,
                                            result: Some(result),
                                            comment: String::new(),
                                        }
                                    })
                                    .collect::<Vec<_>>();
                                jani_model.system.syncs.extend(to_add);
                            }

                            // If original action did not appear in syncs it means that it does not sync between automata.
                            // We still want to keep track of it esplicitely.
                            if jani_model.system.syncs.iter().all(|sync| {
                                sync.synchronise[e_idx]
                                    .as_ref()
                                    .is_none_or(|a| *a != edge_action)
                            }) {
                                let mut synchronise = vec![None; jani_model.system.elements.len()];
                                synchronise[e_idx] = Some(action.clone());
                                // ensure result is unique
                                let result = action.clone() + Self::GEN + &idx.to_string();
                                idx += 1;
                                jani_model.system.syncs.push(Sync {
                                    synchronise,
                                    result: Some(result),
                                    comment: String::new(),
                                });
                            }
                        }
                    }
                }
            }
            // Replace edges with new ones
            automaton.edges = new_edges;
        }
    }

    fn add_global_var(
        &mut self,
        pgb: &mut ProgramGraphBuilder,
        var: &VariableDeclaration,
    ) -> anyhow::Result<()> {
        // TODO WARN FIXME: in JANI initial values are random?
        let init = var
            .initial_value
            .as_ref()
            .ok_or_else(|| anyhow!("missing initial value"))
            .and_then(|expr| self.build_expression(expr, &HashMap::new(), None))?;
        // .unwrap_or_else(|| {
        //     PgExpression::Const(match &var.r#type {
        //         parser::Type::Basic(basic_type) => match basic_type {
        //             parser::BasicType::Bool => scan_core::Val::Boolean(false),
        //             parser::BasicType::Int => scan_core::Val::Integer(0),
        //             parser::BasicType::Real => scan_core::Val::Float(0f64),
        //         },
        //         parser::Type::Bounded(_bounded_type) => todo!(),
        //         parser::Type::Clock(_) => todo!(),
        //         parser::Type::Continuous(_) => todo!(),
        //     })
        // });
        let t = init.r#type()?;
        let var_id = pgb.new_var(init)?;
        self.global_vars.insert(var.name.clone(), (var_id, t));
        Ok(())
    }

    fn add_global_constant(&mut self, c: &ConstantDeclaration) -> anyhow::Result<()> {
        // TODO WARN FIXME: in JANI initial values are random?
        let val = c
            .value
            .as_ref()
            .and_then(|expr| {
                self.build_expression(expr, &HashMap::new(), None)
                    .and_then(|e| e.eval_constant().map_err(|err| anyhow!(err)))
                    .ok()
            })
            .ok_or_else(|| anyhow!("missing initial value"))?;
        // .unwrap_or_else(|| match &c.r#type {
        //     parser::Type::Basic(basic_type) => match basic_type {
        //         parser::BasicType::Bool => scan_core::Val::Boolean(false),
        //         parser::BasicType::Int => scan_core::Val::Integer(0),
        //         parser::BasicType::Real => scan_core::Val::Float(0f64),
        //     },
        //     parser::Type::Bounded(_bounded_type) => todo!(),
        //     parser::Type::Clock(_) => todo!(),
        //     parser::Type::Continuous(_) => todo!(),
        // });
        self.global_constants.insert(c.name.clone(), val);
        Ok(())
    }

    fn add_local_var(
        &self,
        pgb: &mut ProgramGraphBuilder,
        var: &VariableDeclaration,
        local_vars: &mut HashMap<String, (Var, Type)>,
    ) -> anyhow::Result<()> {
        // TODO WARN FIXME: in JANI initial values are random?
        let init = var
            .initial_value
            .as_ref()
            .and_then(|expr| self.build_expression(expr, local_vars, None).ok())
            .ok_or_else(|| anyhow!("missing initial value"))?;
        // .unwrap_or_else(|| {
        //     PgExpression::Const(match &var.r#type {
        //         parser::Type::Basic(basic_type) => match basic_type {
        //             parser::BasicType::Bool => scan_core::Val::Boolean(false),
        //             parser::BasicType::Int => scan_core::Val::Integer(0),
        //             parser::BasicType::Real => scan_core::Val::Float(0f64),
        //         },
        //         parser::Type::Bounded(_bounded_type) => todo!(),
        //         parser::Type::Clock(_) => todo!(),
        //         parser::Type::Continuous(_) => todo!(),
        //     })
        // });
        let t = init.r#type()?;
        let var_id = pgb.new_var(init)?;
        local_vars.insert(var.name.clone(), (var_id, t));
        Ok(())
    }

    fn data(self, jani_model: Model) -> JaniModelData {
        JaniModelData {
            actions: self
                .system_actions
                .into_iter()
                .map(|(name, action)| (action, name))
                .collect::<HashMap<_, _>>(),
            ports: self
                .global_vars
                .into_iter()
                .map(|(name, (_, t))| (name, t))
                .collect(),
            guarantees: jani_model
                .properties
                .into_iter()
                .map(|prop| prop.name)
                .collect(),
        }
    }

    fn build_automaton(
        &mut self,
        jani_model: &Model,
        pgb: &mut ProgramGraphBuilder,
        automaton: &Automaton,
        init: Action,
        e_idx: usize,
    ) -> anyhow::Result<()> {
        // Add local variables
        let mut local_vars: HashMap<String, (Var, Type)> = HashMap::new();
        let mut locations: HashMap<String, scan_core::program_graph::Location> = HashMap::new();
        let mut rng_actions = HashSet::new();
        let pg_initial = pgb.new_initial_location();
        let rng = pgb.new_var(PgExpression::from(0.)).expect("new var");
        pgb.add_effect(init, rng, PgExpression::RandFloat(0., 1.))
            .expect("add effect");
        // Add locations
        for location in &automaton.locations {
            self.build_location(jani_model, pgb, location, e_idx, &mut locations)
                .with_context(|| format!("failed building location: {}", &location.name))?;
        }
        // Connect initial location of PG with initial location(s) of the JANI model
        for initial in &automaton.initial_locations {
            let jani_initial = locations
                .get(initial)
                .ok_or_else(|| anyhow!("missing initial location {}", initial))?;
            pgb.add_transition(pg_initial, init, *jani_initial, None)
                .expect("add transition");
        }

        automaton
            .variables
            .iter()
            .try_for_each(|var| self.add_local_var(pgb, var, &mut local_vars))
            .context("failed adding local variables")?;

        // Add edges
        for (n_edge, edge) in automaton.edges.iter().enumerate() {
            self.build_edge(
                jani_model,
                pgb,
                edge,
                e_idx,
                &local_vars,
                &locations,
                &mut rng_actions,
                rng,
            )
            .with_context(|| format!("failed building {n_edge}-th edge for action"))?;
        }
        Ok(())
    }

    fn build_location(
        &mut self,
        jani_model: &Model,
        pgb: &mut ProgramGraphBuilder,
        location: &Location,
        e_idx: usize,
        locations: &mut HashMap<String, scan_core::program_graph::Location>,
    ) -> anyhow::Result<()> {
        let loc = pgb.new_location();
        assert!(locations.insert(location.name.clone(), loc).is_none());
        // For every action that is **NOT** synchronised on this automaton,
        // allow action with no change in state.
        jani_model
            .system
            .syncs
            .iter()
            .filter(|sync| sync.synchronise[e_idx].is_none())
            .for_each(|sync| {
                let result = sync.result.as_ref().expect("result must have name");
                let action = self.system_actions.get(result).expect("system action");
                pgb.add_transition(loc, *action, loc, None).unwrap();
            });
        Ok(())
    }

    fn build_edge(
        &mut self,
        jani_model: &Model,
        pgb: &mut ProgramGraphBuilder,
        edge: &Edge,
        e_idx: usize,
        local_vars: &HashMap<String, (Var, Type)>,
        locations: &HashMap<String, scan_core::program_graph::Location>,
        rng_actions: &mut HashSet<Action>,
        rng: Var,
    ) -> anyhow::Result<()> {
        let pre = locations.get(&edge.location).ok_or(anyhow!(
            "pre-transition location {} not found",
            edge.location
        ))?;
        let guard = edge
            .guard
            .as_ref()
            .map(|guard| self.build_expression(&guard.exp, local_vars, Some(rng)))
            .transpose()
            .with_context(|| {
                format!(
                    "failed to build guard with expression {:?}",
                    edge.guard.as_ref().map(|g| &g.exp)
                )
            })?;
        // There must be only one destination per edge!
        if let [dest] = edge.destinations.as_slice() {
            let post = locations.get(&dest.location).ok_or(anyhow!(
                "post-transition location {} not found",
                dest.location
            ))?;
            jani_model
                .system
                .syncs
                .iter()
                .filter(|sync| {
                    sync.synchronise[e_idx].as_ref().is_some_and(|sync_action| {
                        *edge.action.as_ref().expect("no silent action") == *sync_action
                    })
                })
                .try_for_each(|sync| {
                    let result = sync.result.as_ref().expect("no silent actions generated");
                    let action = self.system_actions.get(result).unwrap();
                    // checks to do this only once per action
                    if dest.probability.is_some() && !rng_actions.contains(action) {
                        pgb.add_effect(*action, rng, PgExpression::RandFloat(0., 1.))
                            .expect("effect");
                        rng_actions.insert(*action);
                    }
                    for assignment in &dest.assignments {
                        let (var, _) = local_vars
                            .get(&assignment.r#ref)
                            .or_else(|| self.global_vars.get(&assignment.r#ref))
                            .ok_or_else(|| anyhow!("unknown id `{}`", &assignment.r#ref))?;
                        let expr = self
                            .build_expression(&assignment.value, local_vars, Some(rng))
                            .context("failed building expression")?;
                        pgb.add_effect(*action, *var, expr)
                            .context("failed adding effect to action")?;
                    }
                    pgb.add_transition(*pre, *action, *post, guard.clone())
                        .context("failed adding transition")
                })?;
        } else {
            panic!("edges should be normalized");
        }
        Ok(())
    }

    fn build_expression(
        &self,
        expr: &Expression,
        local_vars: &HashMap<String, (Var, Type)>,
        rng: Option<Var>,
    ) -> anyhow::Result<PgExpression> {
        match expr {
            Expression::ConstantValue(constant_value) => match constant_value {
                parser::ConstantValue::Boolean(b) => Ok(PgExpression::from(*b)),
                parser::ConstantValue::Constant(constant) => match constant {
                    parser::Constant::Euler => Ok(PgExpression::from(std::f64::consts::E)),
                    parser::Constant::Pi => Ok(PgExpression::from(std::f64::consts::PI)),
                },
                parser::ConstantValue::NumberReal(num) => Ok(PgExpression::from(*num)),
                parser::ConstantValue::NumberInt(num) => Ok(PgExpression::from(*num)),
            },
            Expression::Identifier(id) if id == Self::RNG => rng
                .ok_or_else(|| anyhow!("rng not available"))
                .map(|rng| PgExpression::Var(rng, Type::Float)),
            Expression::Identifier(id) => local_vars
                .get(id)
                .or_else(|| self.global_vars.get(id))
                .map(|(var, t)| PgExpression::Var(*var, t.clone()))
                .or_else(|| {
                    self.global_constants
                        .get(id)
                        .cloned()
                        .map(PgExpression::Const)
                })
                .ok_or_else(|| anyhow!("unknown id `{id}`")),
            Expression::IfThenElse {
                op,
                r#if,
                then,
                r#else,
            } => {
                let _if = self.build_expression(r#if, local_vars, rng)?;
                let _then = self.build_expression(then, local_vars, rng)?;
                let _else = self.build_expression(r#else, local_vars, rng)?;
                match op {
                    parser::IteOp::Ite => todo!(),
                }
            }
            Expression::Bool { op, left, right } => {
                let left = self.build_expression(left, local_vars, rng)?;
                let right = self.build_expression(right, local_vars, rng)?;
                match op {
                    BoolOp::And => PgExpression::and(vec![left, right]),
                    BoolOp::Or => PgExpression::or(vec![left, right]),
                }
                .map_err(|err| err.into())
            }
            Expression::Neg { op, exp } => {
                let exp = self.build_expression(exp, local_vars, rng)?;
                match op {
                    parser::NegOp::Neg => PgExpression::not(exp).map_err(|err| err.into()),
                }
            }
            Expression::EqComp { op, left, right } => {
                let left = self.build_expression(left, local_vars, rng)?;
                let right = self.build_expression(right, local_vars, rng)?;
                if left.r#type()? == right.r#type()?
                    || (matches!(left.r#type()?, Type::Integer | Type::Float)
                        && matches!(right.r#type()?, Type::Integer | Type::Float))
                {
                    match op {
                        parser::EqCompOp::Eq => Ok(PgExpression::Equal(Box::new((left, right)))),
                        parser::EqCompOp::Neq => PgExpression::Equal(Box::new((left, right)))
                            .not()
                            .map_err(|err| err.into()),
                    }
                } else {
                    bail!(TypeError::TypeMismatch)
                }
            }
            Expression::NumComp { op, left, right } => {
                let left = self.build_expression(left, local_vars, rng)?;
                let right = self.build_expression(right, local_vars, rng)?;
                if matches!(left.r#type()?, Type::Integer | Type::Float)
                    && matches!(right.r#type()?, Type::Integer | Type::Float)
                {
                    Ok(match op {
                        parser::NumCompOp::Less => PgExpression::Less(Box::new((left, right))),
                        parser::NumCompOp::Leq => PgExpression::LessEq(Box::new((left, right))),
                        parser::NumCompOp::Greater => {
                            PgExpression::Greater(Box::new((left, right)))
                        }
                        parser::NumCompOp::Geq => PgExpression::GreaterEq(Box::new((left, right))),
                    })
                } else {
                    bail!(TypeError::TypeMismatch)
                }
            }
            Expression::IntOp { op, left, right } => {
                let left = self.build_expression(left, local_vars, rng)?;
                let right = self.build_expression(right, local_vars, rng)?;
                if matches!(left.r#type()?, Type::Integer | Type::Float)
                    && matches!(right.r#type()?, Type::Integer | Type::Float)
                {
                    match op {
                        parser::IntOp::Plus => Ok(PgExpression::Sum(vec![left, right])),
                        parser::IntOp::Minus => Ok(PgExpression::Sum(vec![
                            left,
                            PgExpression::Opposite(Box::new(right)),
                        ])),
                        parser::IntOp::Mult => Ok(PgExpression::Mult(vec![left, right])),
                        parser::IntOp::IntDiv => todo!(),
                    }
                } else {
                    bail!(TypeError::TypeMismatch)
                }
            }
            Expression::RealOp { op, left, right } => {
                let left = self.build_expression(left, local_vars, rng)?;
                let right = self.build_expression(right, local_vars, rng)?;
                if matches!(left.r#type()?, Type::Integer | Type::Float)
                    && matches!(right.r#type()?, Type::Integer | Type::Float)
                {
                    match op {
                        parser::RealOp::Div => Ok(PgExpression::Div(Box::new((left, right)))),
                        parser::RealOp::Pow => todo!(),
                        parser::RealOp::Log => todo!(),
                    }
                } else {
                    bail!(TypeError::TypeMismatch)
                }
            }
            Expression::Real2IntOp { op, exp } => {
                let _exp = self.build_expression(exp, local_vars, rng)?;
                if matches!(_exp.r#type()?, Type::Float) {
                    match op {
                        parser::Real2IntOp::Floor => todo!(),
                        parser::Real2IntOp::Ceil => todo!(),
                    }
                } else {
                    bail!(TypeError::TypeMismatch)
                }
            }
        }
    }

    fn build_property(
        &self,
        prop: &PropertyExpression,
    ) -> anyhow::Result<Either<PgExpression, Mtl<PgExpression>>> {
        match prop {
            PropertyExpression::ConstantValue(constant_value) => {
                Ok(Either::Left(match constant_value {
                    parser::ConstantValue::Boolean(b) => PgExpression::from(*b),
                    parser::ConstantValue::Constant(constant) => match constant {
                        parser::Constant::Euler => PgExpression::from(std::f64::consts::E),
                        parser::Constant::Pi => PgExpression::from(std::f64::consts::PI),
                    },
                    parser::ConstantValue::NumberReal(num) => PgExpression::from(*num),
                    parser::ConstantValue::NumberInt(num) => PgExpression::from(*num),
                }))
            }
            PropertyExpression::Identifier(id) => self
                .global_vars
                .get(id)
                .map(|(var, t)| PgExpression::Var(*var, t.clone()))
                .or_else(|| {
                    self.global_constants
                        .get(id)
                        .cloned()
                        .map(PgExpression::Const)
                })
                .map(Either::Left)
                .ok_or_else(|| anyhow!("unknown id `{id}`")),
            PropertyExpression::IfThenElse {
                op,
                r#if,
                then,
                r#else,
            } => todo!(),
            PropertyExpression::Bool { op, left, right } => {
                let left = self.build_property(left)?.left().expect("expression");
                let right = self.build_property(right)?.left().expect("expression");
                match op {
                    BoolOp::And => PgExpression::and(vec![left, right]).map_err(|err| err.into()),
                    BoolOp::Or => PgExpression::or(vec![left, right]).map_err(|err| err.into()),
                }
                .map(Either::Left)
            }
            PropertyExpression::Neg { op, exp } => {
                let exp = self.build_property(exp)?.left().expect("expression");
                match op {
                    parser::NegOp::Neg => PgExpression::not(exp).map_err(|err| err.into()),
                }
                .map(Either::Left)
            }
            PropertyExpression::EqComp { op, left, right } => {
                let left = self.build_property(left)?.left().expect("expression");
                let right = self.build_property(right)?.left().expect("expression");
                if left.r#type()? == right.r#type()?
                    || (matches!(left.r#type()?, Type::Integer | Type::Float)
                        && matches!(right.r#type()?, Type::Integer | Type::Float))
                {
                    match op {
                        parser::EqCompOp::Eq => Ok(PgExpression::Equal(Box::new((left, right)))),
                        parser::EqCompOp::Neq => PgExpression::Equal(Box::new((left, right)))
                            .not()
                            .map_err(|err| err.into()),
                    }
                    .map(Either::Left)
                } else {
                    bail!(TypeError::TypeMismatch)
                }
            }
            PropertyExpression::NumComp { op, left, right } => {
                let left = self.build_property(left)?.left().expect("expression");
                let right = self.build_property(right)?.left().expect("expression");
                if matches!(left.r#type()?, Type::Integer | Type::Float)
                    && matches!(right.r#type()?, Type::Integer | Type::Float)
                {
                    Ok(Either::Left(match op {
                        parser::NumCompOp::Less => PgExpression::Less(Box::new((left, right))),
                        parser::NumCompOp::Leq => PgExpression::LessEq(Box::new((left, right))),
                        parser::NumCompOp::Greater => {
                            PgExpression::Greater(Box::new((left, right)))
                        }
                        parser::NumCompOp::Geq => PgExpression::GreaterEq(Box::new((left, right))),
                    }))
                } else {
                    bail!(TypeError::TypeMismatch)
                }
            }
            PropertyExpression::IntOp { op, left, right } => {
                let left = self.build_property(left)?.left().expect("expression");
                let right = self.build_property(right)?.left().expect("expression");
                if matches!(left.r#type()?, Type::Integer | Type::Float)
                    && matches!(right.r#type()?, Type::Integer | Type::Float)
                {
                    match op {
                        parser::IntOp::Plus => Ok(PgExpression::Sum(vec![left, right])),
                        parser::IntOp::Minus => Ok(PgExpression::Sum(vec![
                            left,
                            PgExpression::Opposite(Box::new(right)),
                        ])),
                        parser::IntOp::Mult => Ok(PgExpression::Mult(vec![left, right])),
                        parser::IntOp::IntDiv => todo!(),
                    }
                    .map(Either::Left)
                } else {
                    bail!(TypeError::TypeMismatch)
                }
            }
            PropertyExpression::RealOp { op, left, right } => todo!(),
            PropertyExpression::Real2IntOp { op, exp } => todo!(),
            PropertyExpression::Until {
                op,
                left,
                right,
                time_bounds,
            } => {
                let left = self
                    .build_property(left)?
                    .left()
                    .ok_or(anyhow!("unsupported property"))?;
                let right = self
                    .build_property(right)?
                    .left()
                    .ok_or(anyhow!("unsupported property"))?;
                Ok(Either::Right(match op {
                    parser::UntilOp::Until => Mtl::Until(left, right),
                    parser::UntilOp::WeakUntil => todo!(),
                }))
            }
        }
    }
}
