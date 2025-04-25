use std::sync::Arc;

use rand::{SeedableRng, rngs::SmallRng};

use crate::{
    DummyRng, FnExpression, TransitionSystem, Val,
    program_graph::{Action, PgError, PgExpression, ProgramGraph, Var},
};

pub struct PgModel {
    pg: ProgramGraph<SmallRng>,
    rng: SmallRng,
    predicates: Arc<Vec<FnExpression<Var, DummyRng>>>,
}

impl PgModel {
    pub fn new(pg: ProgramGraph<SmallRng>, rng: SmallRng, predicates: Vec<PgExpression>) -> Self {
        let predicates = predicates
            .into_iter()
            .map(Into::<FnExpression<Var, DummyRng>>::into)
            .collect();
        let predicates = Arc::new(predicates);
        Self {
            pg,
            rng,
            predicates,
        }
    }
}

impl Clone for PgModel {
    fn clone(&self) -> Self {
        Self {
            pg: self.pg.clone(),
            rng: SmallRng::from_os_rng(),
            predicates: Arc::clone(&self.predicates),
        }
    }
}

impl TransitionSystem<Action, PgError> for PgModel {
    fn transition(&mut self, _duration: crate::Time) -> Result<Option<Action>, PgError> {
        Ok(self.pg.montecarlo(&mut self.rng))
    }

    fn time(&self) -> crate::Time {
        0
    }

    fn labels(&self) -> Vec<bool> {
        self.predicates
            .iter()
            .map(|p| {
                if let Val::Boolean(b) = self.pg.eval(p) {
                    b
                } else {
                    panic!("non-bool pred")
                }
            })
            .collect()
    }

    fn state(&self) -> impl Iterator<Item = &crate::Val> {
        self.pg.vars()
    }
}
