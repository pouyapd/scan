mod numset;

use crate::{Atom, Oracle, Time};
use hashbrown::HashSet;
use numset::NumSet;
use std::{hash::Hash, sync::Arc};

type DenseTime = (Time, Time);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pmtl<V>
where
    V: Clone,
{
    True,
    False,
    Atom(V),
    And(Vec<Pmtl<V>>),
    Or(Vec<Pmtl<V>>),
    Not(Box<Pmtl<V>>),
    Implies(Box<(Pmtl<V>, Pmtl<V>)>),
    Historically(Box<Pmtl<V>>, Time, Time),
    Previously(Box<Pmtl<V>>, Time, Time),
    Since(Box<(Pmtl<V>, Pmtl<V>)>, Time, Time),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ArcPmtl<V>
where
    V: Clone,
{
    True,
    False,
    Atom(V),
    And(Vec<IdxPmtl<V>>),
    Or(Vec<IdxPmtl<V>>),
    Not(IdxPmtl<V>),
    Implies(IdxPmtl<V>, IdxPmtl<V>),
    Historically(IdxPmtl<V>, Time, Time),
    Previously(IdxPmtl<V>, Time, Time),
    Since(IdxPmtl<V>, IdxPmtl<V>, Time, Time),
}

type IdxPmtl<V> = (Arc<ArcPmtl<V>>, usize);

#[derive(Debug, Clone)]
pub struct StateValuationVector<V: Clone + Eq + Hash> {
    time: DenseTime,
    subformulae: Arc<Vec<IdxPmtl<Atom<V>>>>,
    valuations: Vec<NumSet>,
    outputs: Vec<NumSet>,
}

impl<V> Pmtl<V>
where
    V: Clone + Eq + Hash,
{
    fn set_subformulae(self) -> HashSet<Pmtl<V>> {
        let mut formulae = match &self {
            Pmtl::True | Pmtl::False | Pmtl::Atom(_) => HashSet::new(),
            Pmtl::And(subs) | Pmtl::Or(subs) => HashSet::from_iter(
                subs.iter()
                    .flat_map(|f| f.clone().set_subformulae().into_iter()),
            ),
            Pmtl::Not(subformula)
            | Pmtl::Historically(subformula, _, _)
            | Pmtl::Previously(subformula, _, _) => subformula.as_ref().clone().set_subformulae(),
            Pmtl::Implies(subs) | Pmtl::Since(subs, _, _) => {
                let mut formulae = subs.0.clone().set_subformulae();
                formulae.extend(subs.1.clone().set_subformulae());
                formulae
            }
        };
        formulae.insert(self);
        formulae
    }

    fn subformulae(self) -> Vec<IdxPmtl<V>> {
        let set = self.set_subformulae();
        let mut vec = Vec::from_iter(set);
        vec.sort_unstable_by_key(Self::depth);
        vec.shrink_to_fit();
        let mut idx_vec: Vec<IdxPmtl<V>> = Vec::new();
        for pmtl in &vec {
            let rc_pmtl = match pmtl {
                Pmtl::True => ArcPmtl::True,
                Pmtl::False => ArcPmtl::False,
                Pmtl::Atom(p) => ArcPmtl::Atom(p.clone()),
                Pmtl::And(subs) => {
                    let mut rc_subs = Vec::new();
                    for sub in subs {
                        let idx = vec.iter().position(|f| f == sub).expect("index");
                        rc_subs.push((Arc::clone(&idx_vec[idx].0), idx));
                    }
                    ArcPmtl::And(rc_subs)
                }
                Pmtl::Or(subs) => {
                    let mut rc_subs = Vec::new();
                    for sub in subs {
                        let idx = vec.iter().position(|f| f == sub).expect("index");
                        rc_subs.push((Arc::clone(&idx_vec[idx].0), idx));
                    }
                    ArcPmtl::Or(rc_subs)
                }
                Pmtl::Not(sub) => {
                    let idx = vec.iter().position(|f| f == sub.as_ref()).expect("index");
                    ArcPmtl::Not((Arc::clone(&idx_vec[idx].0), idx))
                }
                Pmtl::Implies(subs) => {
                    let idx_0 = vec.iter().position(|f| f == (&subs.0)).expect("index");
                    let idx_1 = vec.iter().position(|f| f == (&subs.1)).expect("index");
                    ArcPmtl::Implies(
                        (Arc::clone(&idx_vec[idx_0].0), idx_0),
                        (Arc::clone(&idx_vec[idx_1].0), idx_1),
                    )
                }
                Pmtl::Historically(sub, lower_bound, upper_bound) => {
                    let idx = vec.iter().position(|f| f == sub.as_ref()).expect("index");
                    ArcPmtl::Historically(
                        (Arc::clone(&idx_vec[idx].0), idx),
                        *lower_bound,
                        *upper_bound,
                    )
                }
                Pmtl::Previously(sub, lower_bound, upper_bound) => {
                    let idx = vec.iter().position(|f| f == sub.as_ref()).expect("index");
                    ArcPmtl::Previously(
                        (Arc::clone(&idx_vec[idx].0), idx),
                        *lower_bound,
                        *upper_bound,
                    )
                }
                Pmtl::Since(subs, lower_bound, upper_bound) => {
                    let idx_0 = vec.iter().position(|f| f == (&subs.0)).expect("index");
                    let idx_1 = vec.iter().position(|f| f == (&subs.1)).expect("index");
                    ArcPmtl::Since(
                        (Arc::clone(&idx_vec[idx_0].0), idx_0),
                        (Arc::clone(&idx_vec[idx_1].0), idx_1),
                        *lower_bound,
                        *upper_bound,
                    )
                }
            };
            idx_vec.push((Arc::new(rc_pmtl), idx_vec.len()));
        }
        idx_vec
    }
}

impl<V> Pmtl<V>
where
    V: Clone,
{
    fn depth(&self) -> usize {
        match self {
            Pmtl::True | Pmtl::False | Pmtl::Atom(_) => 0,
            Pmtl::And(subs) | Pmtl::Or(subs) => subs.iter().map(Pmtl::depth).max().unwrap_or(0) + 1,
            Pmtl::Not(sub) | Pmtl::Historically(sub, _, _) | Pmtl::Previously(sub, _, _) => {
                sub.depth() + 1
            }
            Pmtl::Implies(subs) | Pmtl::Since(subs, _, _) => subs.0.depth().max(subs.1.depth()) + 1,
        }
    }
}

impl<V: std::fmt::Debug + Clone + Eq + Hash> Oracle<V> for StateValuationVector<V> {
    fn update(&mut self, action: &V, state: &[bool], time: Time) -> bool {
        *self = self.valuation_update(action, state, time);
        self.output(self.subformulae.last().expect("formula"))
    }
}

impl<V: std::fmt::Debug + Clone + Eq + Hash> StateValuationVector<V> {
    pub fn new(formula: Pmtl<Atom<V>>) -> Self {
        let subformulae = Arc::new(formula.subformulae());

        Self {
            // WARN: all Hell brakes loose with time: (0, 0)
            time: (0, 1),
            valuations: Vec::from_iter((0..subformulae.len()).map(|_| NumSet::new())),
            outputs: Vec::from_iter((0..subformulae.len()).map(|_| NumSet::new())),
            subformulae,
        }
    }

    fn output(&self, formula: &IdxPmtl<Atom<V>>) -> bool {
        self.outputs
            .get(formula.1)
            .map(|out| out.contains(self.time))
            .unwrap()
    }

    fn valuation_update(&self, event: &V, state: &[bool], time: Time) -> Self {
        assert!(self.time.0 <= time);
        let mut new_valuation = Self {
            time: (time, self.time.1 + 1),
            subformulae: self.subformulae.clone(),
            valuations: Vec::with_capacity(self.subformulae.len()),
            outputs: Vec::with_capacity(self.subformulae.len()),
        };
        for (formula, idx) in self.subformulae.iter() {
            let formula = formula.as_ref();
            let idx = *idx;
            match formula {
                ArcPmtl::True => {
                    new_valuation.valuations.push(NumSet::full());
                    new_valuation
                        .outputs
                        .push(NumSet::from_range(self.time, new_valuation.time));
                }
                ArcPmtl::False => {
                    new_valuation.valuations.push(NumSet::new());
                    new_valuation.outputs.push(NumSet::new());
                }
                ArcPmtl::Atom(atom) => match atom {
                    Atom::Predicate(p) if state[*p] => {
                        let numset = NumSet::from_range(self.time, new_valuation.time);
                        new_valuation.valuations.push(numset.clone());
                        new_valuation.outputs.push(numset);
                    }
                    Atom::Event(e) if event == e => {
                        let numset = NumSet::from_range(
                            (new_valuation.time.0, new_valuation.time.1 - 1),
                            new_valuation.time,
                        );
                        new_valuation.valuations.push(numset.clone());
                        new_valuation.outputs.push(numset);
                    }
                    _ => {
                        new_valuation.valuations.push(NumSet::new());
                        new_valuation.outputs.push(NumSet::new());
                    }
                },
                ArcPmtl::And(subs) => {
                    let nset = NumSet::intersection(
                        subs.iter()
                            .filter_map(|f| new_valuation.outputs.get(f.1))
                            .cloned(),
                    )
                    .simplify();
                    new_valuation.valuations.push(nset.clone());
                    new_valuation.outputs.push(nset);
                }
                ArcPmtl::Or(subs) => {
                    let nset = subs
                        .iter()
                        .filter_map(|sub| new_valuation.outputs.get(sub.1))
                        .fold(NumSet::new(), |mut union, numset| {
                            union.union(numset);
                            union
                        })
                        .simplify();
                    new_valuation.valuations.push(nset.clone());
                    new_valuation.outputs.push(nset);
                }
                ArcPmtl::Not(subformula) => {
                    let mut nset = new_valuation
                        .outputs
                        .get(subformula.1)
                        .expect("nset")
                        .clone();
                    nset.complement();
                    nset.cut(self.time, new_valuation.time);
                    nset.simplify();
                    new_valuation.valuations.push(nset.clone());
                    new_valuation.outputs.push(nset);
                }
                ArcPmtl::Implies(sub_0, sub_1) => {
                    let out_lhs = new_valuation.outputs.get(sub_0.1).expect("output lhs");
                    let out_rhs = new_valuation.outputs.get(sub_1.1).expect("output rhs");
                    let mut nset = out_lhs.clone();
                    nset.complement();
                    nset.union(out_rhs);
                    nset.cut(self.time, new_valuation.time);
                    nset.simplify();
                    new_valuation.valuations.push(nset.clone());
                    new_valuation.outputs.push(nset);
                }
                ArcPmtl::Historically(sub, lower_bound, upper_bound) => {
                    let mut output_sub = new_valuation.outputs.get(sub.1).expect("nset").clone();
                    output_sub.insert_bound(new_valuation.time);
                    let mut valuation = self.valuations.get(idx).expect("formula").clone();
                    let mut partial_lower_bound = self.time;
                    let mut nset_output = NumSet::new();
                    for (partial_upper_bound, out_sub) in
                        output_sub.bounds().iter().filter(|(ub, _)| self.time < *ub)
                    {
                        assert!(partial_lower_bound < *partial_upper_bound);
                        assert!(self.time < *partial_upper_bound);
                        if !*out_sub {
                            let lower_bound = if *lower_bound > 0 {
                                lower_bound
                                    .checked_add(partial_lower_bound.0)
                                    .map(|ub| (ub, 0))
                                    .unwrap_or((Time::MAX, Time::MAX))
                            } else {
                                partial_lower_bound
                            };
                            let upper_bound = upper_bound
                                .checked_add(partial_upper_bound.0)
                                .map(|ub| (ub, Time::MAX))
                                .unwrap_or((Time::MAX, Time::MAX));
                            valuation.add_interval(lower_bound, upper_bound);
                        }
                        let mut to_add = valuation.clone();
                        to_add.cut(partial_lower_bound, *partial_upper_bound);
                        nset_output.union(&to_add);
                        partial_lower_bound = *partial_upper_bound;
                    }
                    nset_output.complement();
                    nset_output.cut(self.time, new_valuation.time);
                    valuation.cut(self.time, (Time::MAX, Time::MAX));
                    new_valuation.valuations.push(valuation.simplify());
                    new_valuation.outputs.push(nset_output.simplify());
                }
                ArcPmtl::Previously(sub, lower_bound, upper_bound) => {
                    let mut output_sub = new_valuation.outputs.get(sub.1).expect("nset").clone();
                    output_sub.insert_bound(new_valuation.time);
                    let mut valuation = self.valuations.get(idx).expect("formula").clone();
                    let mut partial_lower_bound = self.time;
                    let mut output = NumSet::new();
                    for (partial_upper_bound, out_sub) in
                        output_sub.bounds().iter().filter(|(ub, _)| self.time < *ub)
                    {
                        assert!(partial_lower_bound < *partial_upper_bound);
                        assert!(self.time < *partial_upper_bound);
                        assert!(*partial_upper_bound <= new_valuation.time);
                        if *out_sub {
                            let interval_lower_bound = if *lower_bound > 0 {
                                lower_bound
                                    .checked_add(partial_lower_bound.0)
                                    .map(|ub| (ub, 0))
                                    .unwrap()
                            } else {
                                partial_lower_bound
                            };
                            let interval_upper_bound = upper_bound
                                .checked_add(partial_upper_bound.0)
                                .map(|ub| (ub, Time::MAX))
                                .unwrap_or((Time::MAX, Time::MAX));
                            valuation.add_interval(interval_lower_bound, interval_upper_bound);
                        }
                        let mut to_add = valuation.clone();
                        to_add.cut(partial_lower_bound, *partial_upper_bound);
                        output.union(&to_add);
                        partial_lower_bound = *partial_upper_bound;
                    }
                    valuation.cut(self.time, (Time::MAX, Time::MAX));
                    new_valuation.valuations.push(valuation.simplify());
                    new_valuation.outputs.push(output.simplify());
                }
                ArcPmtl::Since(sub_0, sub_1, lower_bound, upper_bound) => {
                    let mut nset_lhs = new_valuation.outputs.get(sub_0.1).expect("nset").clone();
                    let nset_rhs_orig = new_valuation.outputs.get(sub_1.1).expect("nset");
                    let mut nset_rhs = nset_rhs_orig.clone();
                    nset_rhs.insert_bound(new_valuation.time);
                    nset_lhs.insert_bound(new_valuation.time);
                    nset_rhs.sync(&nset_lhs);
                    nset_lhs.sync(nset_rhs_orig);
                    let mut valuation = self.valuations.get(idx).expect("formula").clone();
                    let mut partial_lower_bound = self.time;
                    let mut nset_output = NumSet::new();
                    for (idx, (partial_upper_bound, out_lhs)) in nset_lhs
                        .bounds()
                        .iter()
                        .enumerate()
                        .filter(|(_, (ub, _))| self.time < *ub)
                    {
                        // since nset_0 and nset_1 are synched:
                        assert_eq!(*partial_upper_bound, nset_rhs.bounds()[idx].0);
                        assert!(partial_lower_bound < *partial_upper_bound);
                        assert!(self.time < *partial_upper_bound);
                        let out_rhs = nset_rhs.bounds()[idx].1;
                        valuation = match (out_lhs, out_rhs) {
                            (true, true) => {
                                let lower_bound = if *lower_bound > 0 {
                                    lower_bound
                                        .checked_add(partial_lower_bound.0)
                                        .map(|ub| (ub, 0))
                                        .unwrap_or((Time::MAX, Time::MAX))
                                } else {
                                    partial_lower_bound
                                };
                                let upper_bound = upper_bound
                                    .checked_add(partial_upper_bound.0)
                                    .map(|ub| (ub, Time::MAX))
                                    .unwrap_or((Time::MAX, Time::MAX));
                                valuation.add_interval(lower_bound, upper_bound);
                                valuation
                            }
                            (true, false) => valuation,
                            (false, true) => {
                                let lower_bound = if *lower_bound > 0 {
                                    lower_bound
                                        .checked_add(partial_upper_bound.0)
                                        .map(|ub| (ub, 0))
                                        .unwrap_or((Time::MAX, Time::MAX))
                                } else {
                                    *partial_upper_bound
                                };
                                let upper_bound = upper_bound
                                    .checked_add(partial_upper_bound.0)
                                    .map(|ub| (ub, Time::MAX))
                                    .unwrap_or((Time::MAX, Time::MAX));
                                NumSet::from_range(lower_bound, upper_bound)
                            }
                            (false, false) => NumSet::new(),
                        };
                        let mut to_add = valuation.clone();
                        to_add.cut(partial_lower_bound, *partial_upper_bound);
                        nset_output.union(&to_add);
                        partial_lower_bound = *partial_upper_bound;
                    }
                    valuation.cut(self.time, (Time::MAX, Time::MAX));
                    new_valuation.valuations.push(valuation.simplify());
                    new_valuation.outputs.push(nset_output.simplify());
                }
            }
        }
        new_valuation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subformulae_1() {
        let subformulae: Vec<IdxPmtl<usize>> =
            Pmtl::Since(Box::new((Pmtl::True, Pmtl::True)), 0, Time::MAX).subformulae();
        assert_eq!(
            subformulae,
            vec![
                (Arc::new(ArcPmtl::True), 0),
                (
                    Arc::new(ArcPmtl::Since(
                        (Arc::new(ArcPmtl::True), 0),
                        (Arc::new(ArcPmtl::True), 0),
                        0,
                        Time::MAX
                    )),
                    1
                ),
            ]
        );
    }

    #[test]
    fn subformulae_2() {
        let subformulae: Vec<IdxPmtl<usize>> =
            Pmtl::Since(Box::new((Pmtl::Atom(0), Pmtl::Atom(0))), 0, Time::MAX).subformulae();
        assert_eq!(
            subformulae,
            vec![
                (Arc::new(ArcPmtl::Atom(0)), 0),
                (
                    Arc::new(ArcPmtl::Since(
                        (Arc::new(ArcPmtl::Atom(0)), 0),
                        (Arc::new(ArcPmtl::Atom(0)), 0),
                        0,
                        Time::MAX
                    )),
                    1
                ),
            ]
        );
    }

    // #[test]
    // fn subformulae_3() {
    //     let subformulae: Vec<IdxPmtl<usize>> = Pmtl::And(vec![
    //         Pmtl::Atom(0),
    //         Pmtl::Not(Arc::new(Pmtl::Atom(0))),
    //         Pmtl::True,
    //     ])
    //     .subformulae();
    //     assert_eq!(subformulae.len(), 4);
    //     assert!(matches!(subformulae[0], IdxPmtl::Atom(0) | IdxPmtl::True));
    //     assert!(matches!(subformulae[1], IdxPmtl::Atom(0) | IdxPmtl::True));
    //     assert_eq!(subformulae[2], Pmtl::Not(Arc::new(Pmtl::Atom(0))));
    // }

    #[test]
    fn since_1() {
        let formula = Pmtl::Since(
            Box::new((
                Pmtl::Atom(Atom::Predicate(0)),
                Pmtl::Atom(Atom::Predicate(1)),
            )),
            0,
            Time::MAX,
        );
        let mut state = StateValuationVector::new(formula);
        assert!(!state.update(&0, &[false, true], 0));
        assert!(!state.update(&0, &[false, true], 1));
        assert!(state.update(&0, &[true, true], 2));
        assert!(state.update(&0, &[true, true], 3));
        assert!(state.update(&0, &[true, false], 4));
        assert!(!state.update(&0, &[false, false], 5));
    }

    #[test]
    fn since_2() {
        let formula = Pmtl::Since(
            Box::new((
                Pmtl::Atom(Atom::Predicate(0)),
                Pmtl::Atom(Atom::Predicate(1)),
            )),
            0,
            2,
        );
        let mut state = StateValuationVector::new(formula);
        assert!(!state.update(&0, &[false, true], 0));
        assert!(!state.update(&0, &[false, true], 1));
        assert!(state.update(&0, &[true, true], 2));
        assert!(state.update(&0, &[true, false], 3));
        assert!(state.update(&0, &[true, false], 4));
        assert!(!state.update(&0, &[true, false], 5));
    }

    #[test]
    fn since_3() {
        let formula = Pmtl::Since(
            Box::new((
                Pmtl::Atom(Atom::Predicate(0)),
                Pmtl::Atom(Atom::Predicate(1)),
            )),
            1,
            2,
        );
        let mut state = StateValuationVector::new(formula);
        assert!(!state.update(&0, &[false, true], 0));
        assert!(!state.update(&0, &[false, true], 1));
        assert!(state.update(&0, &[true, true], 2));
        assert!(state.update(&0, &[true, false], 3));
        assert!(state.update(&0, &[true, false], 4));
        assert!(!state.update(&0, &[true, false], 5));
    }

    #[test]
    fn since_4() {
        let formula = Pmtl::Since(
            Box::new((
                Pmtl::Atom(Atom::Predicate(0)),
                Pmtl::Atom(Atom::Predicate(1)),
            )),
            1,
            2,
        );
        let mut state = StateValuationVector::new(formula);
        assert!(!state.update(&0, &[false, true], 0));
        assert!(!state.update(&0, &[false, true], 1));
        assert!(!state.update(&0, &[false, true], 2));
        assert!(!state.update(&0, &[true, true], 2));
        assert!(state.update(&0, &[true, false], 3));
        assert!(state.update(&0, &[true, false], 4));
        assert!(!state.update(&0, &[true, false], 5));
    }

    #[test]
    fn historically() {
        let formula = Pmtl::Historically(Box::new(Pmtl::Atom(Atom::Predicate(0))), 1, 2);
        let mut state = StateValuationVector::new(formula);
        assert!(state.update(&0, &[false], 0));
        assert!(state.update(&0, &[false], 0));
        assert!(!state.update(&0, &[true], 1));
        assert!(!state.update(&0, &[true], 2));
        assert!(state.update(&0, &[true], 3));
        assert!(state.update(&0, &[false], 3));
        assert!(!state.update(&0, &[true], 4));
    }

    #[test]
    fn previously() {
        let formula = Pmtl::Previously(Box::new(Pmtl::Atom(Atom::Predicate(0))), 1, 2);
        let mut state = StateValuationVector::new(formula);
        assert!(!state.update(&0, &[false], 0));
        assert!(!state.update(&0, &[false], 0));
        assert!(state.update(&0, &[true], 1));
        assert!(state.update(&0, &[false], 2));
        assert!(state.update(&0, &[false], 3));
        assert!(state.update(&0, &[false], 3));
        assert!(state.update(&0, &[true], 4));
    }
}
