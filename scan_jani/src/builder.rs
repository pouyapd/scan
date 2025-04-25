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
use std::{collections::HashMap, ops::Not};

#[derive(Clone)]
pub struct JaniModelData {
    pub actions: HashMap<Action, String>,
    pub ports: Vec<(String, Type)>,
    pub guarantees: Vec<String>,
}

pub(crate) fn build(mut jani_model: Model) -> anyhow::Result<(PgModel, MtlOracle, JaniModelData)> {
    let mut builder = JaniBuilder::default();
    normalize(&mut jani_model);
    let (pg_model, oracle) = builder.build(&jani_model)?;
    let data = builder.data();
    Ok((pg_model, oracle, data))
}

// An action in JANI doesn not carry effects,
// so we need to duplicate actions until each one has unique effects.
// The modified model is such that:
//
// - Every action has a unique set of assignments.
// - Every edge has a unique location.
// - Syncs are updated with new actions.
fn normalize(jani_model: &mut Model) {
    // index is global so there is no risk of name-clash
    let mut idx = 0;
    let rng = Expression::Identifier(String::from("__rng__"));
    for automaton in &mut jani_model.automata {
        let mut new_edges = Vec::new();
        for edge in &mut automaton.edges {
            let orig_action = edge.action.clone();
            let mut prob = Expression::ConstantValue(parser::ConstantValue::NumberReal(0f64));
            for dest in &mut edge.destinations {
                // If edge has assignments, create new action
                let action = if dest.assignments.is_empty() {
                    if dest.probability.is_some() && edge.action.is_none() {
                        // Cannot be silent action because needs to randomize
                        edge.action = Some(String::from("__auto_gen__") + &idx.to_string());
                        idx += 1;
                    }
                    // Can be silent action or whatever
                    edge.action.clone()
                } else {
                    let new_action =
                        edge.action.clone().unwrap_or_default() + "__auto_gen__" + &idx.to_string();
                    idx += 1;
                    Some(new_action)
                };
                // Add probability to guard
                let mut guard_exp = edge.guard.as_ref().map(|guard| guard.exp.clone());
                if let Some(p) = dest.probability.as_ref() {
                    let lower_bound = Expression::NumComp {
                        op: NumCompOp::Leq,
                        left: Box::new(prob.clone()),
                        right: Box::new(rng.clone()),
                    };
                    prob = Expression::IntOp {
                        op: parser::IntOp::Plus,
                        left: Box::new(prob),
                        right: Box::new(p.exp.clone()),
                    };
                    let upper_bound = Expression::NumComp {
                        op: NumCompOp::Less,
                        left: Box::new(rng.clone()),
                        right: Box::new(prob.clone()),
                    };
                    guard_exp = guard_exp.map_or(
                        Some(Expression::Bool {
                            op: BoolOp::And,
                            left: Box::new(lower_bound.clone()),
                            right: Box::new(upper_bound.clone()),
                        }),
                        |g| {
                            Some(Expression::Bool {
                                op: BoolOp::And,
                                left: Box::new(Expression::Bool {
                                    op: BoolOp::And,
                                    left: Box::new(lower_bound),
                                    right: Box::new(upper_bound),
                                }),
                                right: Box::new(g),
                            })
                        },
                    );
                }
                let new_edge = Edge {
                    location: edge.location.clone(),
                    action: action.clone(),
                    guard: guard_exp.map(|exp| Guard {
                        exp,
                        comment: String::new(),
                    }),
                    destinations: vec![dest.clone()],
                    comment: String::new(),
                };
                new_edges.push(new_edge);

                // Update syncs with new action (has to synchronise like original one)
                // NOTE: you cannot synchronise the silent action!
                if let Some(ref orig_action) = orig_action {
                    for i in jani_model
                        .system
                        .elements
                        .iter()
                        .enumerate()
                        .filter_map(|(i, e)| (e.automaton == automaton.name).then_some(i))
                    {
                        let mut to_add = Vec::new();
                        for sync in &jani_model.system.syncs {
                            if sync.synchronise[i].as_ref() == Some(orig_action) {
                                let mut synchronise = sync.synchronise.clone();
                                synchronise[i] = action.clone();
                                // Generate new unique result action
                                let new_result = sync.result.clone().unwrap_or_default()
                                    + "__auto_gen__"
                                    + &idx.to_string();
                                idx += 1;
                                to_add.push(Sync {
                                    synchronise,
                                    result: Some(new_result),
                                    comment: String::new(),
                                });
                            }
                        }
                        // If original action did not appear in syncs it means that it does not sync between automata.
                        // We still want to keep track of it esplicitely.
                        if to_add.is_empty() && action.is_some() {
                            let mut synchronise = vec![None; jani_model.system.elements.len()];
                            synchronise[i] = action.clone();
                            // ensure result is unique
                            let new_result = action.clone().unwrap_or_default()
                                + "__auto_gen__"
                                + &idx.to_string();
                            to_add.push(Sync {
                                synchronise,
                                result: Some(new_result),
                                comment: String::new(),
                            });
                        }
                        // Add generated syncs
                        jani_model.system.syncs.extend(to_add);
                    }
                }
            }
        }
        // Replace edges with new ones
        automaton.edges = new_edges;
    }
}

#[derive(Default)]
struct JaniBuilder {
    locations: HashMap<String, program_graph::Location>,
    system_actions: HashMap<String, program_graph::Action>,
    global_vars: HashMap<String, (Var, Type)>,
    global_constants: HashMap<String, Val>,
}

impl JaniBuilder {
    pub(crate) fn build(&mut self, jani_model: &Model) -> anyhow::Result<(PgModel, MtlOracle)> {
        let mut pgb = ProgramGraphBuilder::new();

        jani_model
            .system
            .syncs
            .iter()
            .flat_map(|sync| &sync.result)
            .for_each(|action| {
                if !self.system_actions.contains_key(action) {
                    let action_id = pgb.new_action();
                    let prev = self.system_actions.insert(action.clone(), action_id);
                    assert!(prev.is_none(), "checked by above if condition");
                }
            });

        jani_model
            .variables
            .iter()
            .try_for_each(|var| self.add_global_var(&mut pgb, var))?;
        jani_model
            .constants
            .iter()
            .try_for_each(|c| self.add_global_constant(c))?;

        for (e_idx, element) in jani_model.system.elements.iter().enumerate() {
            let id = &element.automaton;
            let automaton = jani_model
                .automata
                .iter()
                .find(|a| a.name == *id)
                .ok_or(anyhow!("element '{id}' is not a known automaton"))?;
            self.build_automaton(jani_model, &mut pgb, automaton, e_idx)
                .with_context(|| format!("failed to build automaton '{id}'"))?;
        }

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

        // Finalize, build and return everything
        let pg = pgb.build();
        let pg_model = PgModel::new(pg, SmallRng::from_os_rng(), predicates);

        Ok((pg_model, oracle))
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
            .and_then(|expr| self.build_expression(expr, &HashMap::new(), None).ok())
            .unwrap_or_else(|| {
                PgExpression::Const(match &var.r#type {
                    parser::Type::Basic(basic_type) => match basic_type {
                        parser::BasicType::Bool => scan_core::Val::Boolean(false),
                        parser::BasicType::Int => scan_core::Val::Integer(0),
                        parser::BasicType::Real => scan_core::Val::Float(0f64),
                    },
                    parser::Type::Bounded(_bounded_type) => todo!(),
                    parser::Type::Clock(_) => todo!(),
                    parser::Type::Continuous(_) => todo!(),
                })
            });
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
            .and_then(|expr| self.build_expression(expr, &HashMap::new(), None).ok())
            .unwrap_or_else(|| {
                PgExpression::Const(match &c.r#type {
                    parser::Type::Basic(basic_type) => match basic_type {
                        parser::BasicType::Bool => scan_core::Val::Boolean(false),
                        parser::BasicType::Int => scan_core::Val::Integer(0),
                        parser::BasicType::Real => scan_core::Val::Float(0f64),
                    },
                    parser::Type::Bounded(_bounded_type) => todo!(),
                    parser::Type::Clock(_) => todo!(),
                    parser::Type::Continuous(_) => todo!(),
                })
            })
            .eval_constant()?;
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
            .unwrap_or_else(|| {
                PgExpression::Const(match &var.r#type {
                    parser::Type::Basic(basic_type) => match basic_type {
                        parser::BasicType::Bool => scan_core::Val::Boolean(false),
                        parser::BasicType::Int => scan_core::Val::Integer(0),
                        parser::BasicType::Real => scan_core::Val::Float(0f64),
                    },
                    parser::Type::Bounded(_bounded_type) => todo!(),
                    parser::Type::Clock(_) => todo!(),
                    parser::Type::Continuous(_) => todo!(),
                })
            });
        let t = init.r#type()?;
        let var_id = pgb.new_var(init)?;
        local_vars.insert(var.name.clone(), (var_id, t));
        Ok(())
    }

    fn data(self) -> JaniModelData {
        JaniModelData {
            actions: self
                .system_actions
                .into_iter()
                .map(|(name, action)| (action, name))
                .collect::<HashMap<_, _>>(),
            ports: Vec::new(),
            guarantees: Vec::new(),
        }
    }

    fn build_automaton(
        &mut self,
        jani_model: &Model,
        pgb: &mut ProgramGraphBuilder,
        automaton: &Automaton,
        e_idx: usize,
    ) -> anyhow::Result<()> {
        let mut local_vars: HashMap<String, (Var, Type)> = HashMap::new();
        let rng = pgb.new_var(PgExpression::RandFloat(0., 1.))?;
        automaton
            .variables
            .iter()
            .try_for_each(|var| self.add_local_var(pgb, var, &mut local_vars))
            .context("failed adding local variables")?;
        // Add locations
        for location in &automaton.locations {
            self.build_location(jani_model, pgb, location, e_idx)
                .with_context(|| format!("failed building location: {}", &location.name))?;
        }
        // Connect initial location of PG with initial location(s) of the JANI model
        let pg_initial = pgb.new_initial_location();
        for initial in &automaton.initial_locations {
            let jani_initial = *self
                .locations
                .get(initial)
                .ok_or_else(|| anyhow!("missing initial location {}", initial))?;
            pgb.add_autonomous_transition(pg_initial, jani_initial, None)
                .expect("add transition");
        }
        // Add edges
        for edge in &automaton.edges {
            self.build_edge(jani_model, pgb, edge, e_idx, &local_vars, rng)
                .with_context(|| {
                    format!(
                        "failed building edge for action: {}",
                        edge.action.clone().unwrap_or(String::from("`silent`"))
                    )
                })?;
        }
        Ok(())
    }

    fn build_location(
        &mut self,
        jani_model: &Model,
        pgb: &mut ProgramGraphBuilder,
        location: &Location,
        e_idx: usize,
    ) -> anyhow::Result<()> {
        let loc = pgb.new_location();
        assert!(self.locations.insert(location.name.clone(), loc).is_none());
        // For every action that is **NOT** synchronised on this automaton,
        // allow action with no change in state.
        for sync in jani_model
            .system
            .syncs
            .iter()
            .filter(|sync| sync.synchronise[e_idx].is_none())
        {
            if let Some(ref action) = sync.result {
                let action_id = self.system_actions.get(action).unwrap();
                pgb.add_transition(loc, *action_id, loc, None).unwrap();
            } else {
                pgb.add_autonomous_transition(loc, loc, None).unwrap();
            }
        }
        Ok(())
    }

    fn build_edge(
        &mut self,
        jani_model: &Model,
        pgb: &mut ProgramGraphBuilder,
        edge: &Edge,
        e_idx: usize,
        local_vars: &HashMap<String, (Var, Type)>,
        rng: Var,
    ) -> anyhow::Result<()> {
        let pre = *self.locations.get(&edge.location).ok_or(anyhow!(
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
            let post = &dest.location;
            let post = *self.locations.get(post).ok_or(anyhow!(
                "post-transition location {} not found",
                edge.location
            ))?;
            for sync in jani_model.system.syncs.iter().filter(|sync| {
                sync.synchronise[e_idx].as_ref().is_some_and(|sync_action| {
                    edge.action
                        .as_ref()
                        .is_some_and(|edge_action| sync_action == edge_action)
                })
            }) {
                if let Some(ref action) = sync.result {
                    let action = self.system_actions.get(action).unwrap();
                    // TODO: check to do this only once per action
                    pgb.add_effect(*action, rng, PgExpression::RandFloat(0., 1.))
                        .expect("effect");
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
                    pgb.add_transition(pre, *action, post, guard.clone())
                        .context("failed adding transition")?;
                } else {
                    assert!(
                        dest.assignments.is_empty(),
                        "silent action has no assignments"
                    );
                    pgb.add_autonomous_transition(pre, post, guard.clone())
                        .context("failed adding autonomous transition")?;
                }
            }
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
            Expression::Identifier(id) if id == "__rng__" => rng
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
                if matches!(left.r#type()?, Type::Float) && matches!(right.r#type()?, Type::Float) {
                    match op {
                        parser::RealOp::Div => todo!(),
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
