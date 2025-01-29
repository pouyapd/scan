mod numset;

use crate::channel_system::Event;
use crate::{Atom, Time};
use numset::NumSet;
use std::collections::HashSet;
use std::{hash::Hash, sync::Arc};

type DenseTime = (Time, Time);

/// A Past-time Metric Temporal Logic (PMTL) formula.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pmtl<V>
where
    V: Clone,
{
    /// The true formula.
    True,
    /// The false formula.
    False,
    /// An atomic formula.
    Atom(V),
    /// Logical disjunction of a list of formulae.
    And(Vec<Pmtl<V>>),
    /// Logical conjunction of a list of formulae.
    Or(Vec<Pmtl<V>>),
    /// Logical negation of a formula.
    Not(Box<Pmtl<V>>),
    /// Logical implication of a antecedent formula and a consequent formula.
    Implies(Box<(Pmtl<V>, Pmtl<V>)>),
    /// Temporal historical predicate over a formula (with bounds).
    Historically(Box<Pmtl<V>>, Time, Time),
    /// Temporal previously predicate over a formula (with bounds).
    Once(Box<Pmtl<V>>, Time, Time),
    /// Temporal since predicate over a formula (with bounds).
    Since(Box<(Pmtl<V>, Pmtl<V>)>, Time, Time),
}

impl<V> Pmtl<V>
where
    V: Clone + Eq,
{
    fn is_same(&self, value: &ArcPmtl<V>) -> bool {
        match value {
            ArcPmtl::True => matches!(self, &Pmtl::True),
            ArcPmtl::False => matches!(self, &Pmtl::False),
            ArcPmtl::Atom(v) => {
                if let Pmtl::Atom(self_v) = self {
                    v == self_v
                } else {
                    false
                }
            }
            ArcPmtl::And(subs) => {
                if let Pmtl::And(self_subs) = self {
                    subs.len() == self_subs.len()
                        && self_subs
                            .iter()
                            .zip(subs)
                            .all(|(s, (v, _))| s.is_same(v.as_ref()))
                } else {
                    false
                }
            }
            ArcPmtl::Or(subs) => {
                if let Pmtl::Or(self_subs) = self {
                    subs.len() == self_subs.len()
                        && self_subs
                            .iter()
                            .zip(subs)
                            .all(|(s, (v, _))| s.is_same(v.as_ref()))
                } else {
                    false
                }
            }
            ArcPmtl::Not((sub, _)) => {
                if let Pmtl::Not(self_sub) = self {
                    self_sub.is_same(sub.as_ref())
                } else {
                    false
                }
            }
            ArcPmtl::Implies((lhs, _), (rhs, _)) => {
                if let Pmtl::Implies(self_sub) = self {
                    self_sub.0.is_same(lhs.as_ref()) && self_sub.1.is_same(rhs.as_ref())
                } else {
                    false
                }
            }
            ArcPmtl::Historically((sub, _), lower_bound, upper_bound) => {
                if let Pmtl::Historically(self_sub, self_lower_bound, self_upper_bound) = self {
                    self_sub.is_same(sub.as_ref())
                        && lower_bound == self_lower_bound
                        && upper_bound == self_upper_bound
                } else {
                    false
                }
            }
            ArcPmtl::Previously((sub, _), lower_bound, upper_bound) => {
                if let Pmtl::Once(self_sub, self_lower_bound, self_upper_bound) = self {
                    self_sub.is_same(sub.as_ref())
                        && lower_bound == self_lower_bound
                        && upper_bound == self_upper_bound
                } else {
                    false
                }
            }
            ArcPmtl::Since((lhs, _), (rhs, _), lower_bound, upper_bound) => {
                if let Pmtl::Since(self_sub, self_lower_bound, self_upper_bound) = self {
                    self_sub.0.is_same(lhs.as_ref())
                        && self_sub.1.is_same(rhs.as_ref())
                        && lower_bound == self_lower_bound
                        && upper_bound == self_upper_bound
                } else {
                    false
                }
            }
        }
    }
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

/// An oracle for PMTL properties over timed, dense traces.
#[derive(Debug, Clone)]
pub struct PmtlOracle {
    time: DenseTime,
    assumes: Vec<usize>,
    guarantees: Vec<usize>,
    subformulae: Arc<Vec<ArcPmtl<Atom>>>,
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
            | Pmtl::Once(subformula, _, _) => subformula.as_ref().clone().set_subformulae(),
            Pmtl::Implies(subs) | Pmtl::Since(subs, _, _) => {
                let mut formulae = subs.0.clone().set_subformulae();
                formulae.extend(subs.1.clone().set_subformulae());
                formulae
            }
        };
        formulae.insert(self);
        formulae
    }
}

fn subformulae<V: Clone + Eq + Hash>(set: HashSet<Pmtl<V>>) -> Vec<ArcPmtl<V>> {
    let mut vec = Vec::from_iter(set);
    vec.sort_unstable_by_key(Pmtl::depth);
    vec.shrink_to_fit();
    let mut idx_vec: Vec<ArcPmtl<V>> = Vec::new();
    for pmtl in vec {
        let arc_pmtl = match pmtl {
            Pmtl::True => ArcPmtl::True,
            Pmtl::False => ArcPmtl::False,
            Pmtl::Atom(p) => ArcPmtl::Atom(p),
            Pmtl::And(subs) => {
                let mut arc_subs = Vec::new();
                for sub in subs {
                    let idx = idx_vec.iter().position(|f| sub.is_same(f)).expect("index");
                    arc_subs.push((Arc::new(idx_vec[idx].clone()), idx));
                }
                ArcPmtl::And(arc_subs)
            }
            Pmtl::Or(subs) => {
                let mut arc_subs = Vec::new();
                for sub in subs {
                    let idx = idx_vec.iter().position(|f| sub.is_same(f)).expect("index");
                    arc_subs.push((Arc::new(idx_vec[idx].clone()), idx));
                }
                ArcPmtl::Or(arc_subs)
            }
            Pmtl::Not(sub) => {
                let idx = idx_vec.iter().position(|f| sub.is_same(f)).expect("index");
                ArcPmtl::Not((Arc::new(idx_vec[idx].clone()), idx))
            }
            Pmtl::Implies(subs) => {
                let idx_0 = idx_vec
                    .iter()
                    .position(|f| subs.0.is_same(f))
                    .expect("index");
                let idx_1 = idx_vec
                    .iter()
                    .position(|f| subs.1.is_same(f))
                    .expect("index");
                ArcPmtl::Implies(
                    (Arc::new(idx_vec[idx_0].clone()), idx_0),
                    (Arc::new(idx_vec[idx_1].clone()), idx_1),
                )
            }
            Pmtl::Historically(sub, lower_bound, upper_bound) => {
                let idx = idx_vec.iter().position(|f| sub.is_same(f)).expect("index");
                ArcPmtl::Historically(
                    (Arc::new(idx_vec[idx].clone()), idx),
                    lower_bound,
                    upper_bound,
                )
            }
            Pmtl::Once(sub, lower_bound, upper_bound) => {
                let idx = idx_vec.iter().position(|f| sub.is_same(f)).expect("index");
                ArcPmtl::Previously(
                    (Arc::new(idx_vec[idx].clone()), idx),
                    lower_bound,
                    upper_bound,
                )
            }
            Pmtl::Since(subs, lower_bound, upper_bound) => {
                let idx_0 = idx_vec
                    .iter()
                    .position(|f| subs.0.is_same(f))
                    .expect("index");
                let idx_1 = idx_vec
                    .iter()
                    .position(|f| subs.1.is_same(f))
                    .expect("index");
                ArcPmtl::Since(
                    (Arc::new(idx_vec[idx_0].clone()), idx_0),
                    (Arc::new(idx_vec[idx_1].clone()), idx_1),
                    lower_bound,
                    upper_bound,
                )
            }
        };
        idx_vec.push(arc_pmtl);
    }
    idx_vec
}

impl<V> Pmtl<V>
where
    V: Clone,
{
    fn depth(&self) -> usize {
        match self {
            Pmtl::True | Pmtl::False | Pmtl::Atom(_) => 0,
            Pmtl::And(subs) | Pmtl::Or(subs) => subs.iter().map(Pmtl::depth).max().unwrap_or(0) + 1,
            Pmtl::Not(sub) | Pmtl::Historically(sub, _, _) | Pmtl::Once(sub, _, _) => {
                sub.depth() + 1
            }
            Pmtl::Implies(subs) | Pmtl::Since(subs, _, _) => subs.0.depth().max(subs.1.depth()) + 1,
        }
    }
}

impl PmtlOracle {
    /// Creates an oracle from assumes and guarantees PMTL formulae.
    pub fn new(assumes: &[Pmtl<Atom>], guarantees: &[Pmtl<Atom>]) -> Self {
        let set = HashSet::from_iter(
            assumes
                .iter()
                .chain(guarantees)
                .flat_map(|f| f.clone().set_subformulae().into_iter()),
        );
        let subformulae = Arc::new(subformulae(set));
        let assumes = assumes
            .iter()
            .map(|a| {
                subformulae
                    .iter()
                    .position(|f| a.is_same(f))
                    .expect("find assume")
            })
            .collect();
        let guarantees = guarantees
            .iter()
            .map(|g| {
                subformulae
                    .iter()
                    .position(|f| g.is_same(f))
                    .expect("find assume")
            })
            .collect();

        Self {
            // WARN: all Hell brakes loose with time: (0, 0)
            time: (0, 1),
            assumes,
            guarantees,
            valuations: Vec::from_iter((0..subformulae.len()).map(|_| NumSet::new())),
            outputs: Vec::from_iter((0..subformulae.len()).map(|_| NumSet::new())),
            subformulae,
        }
    }

    #[inline(always)]
    fn formula_output(&self, formula: usize) -> bool {
        self.outputs[formula].contains(self.time)
    }

    pub(crate) fn output(&self) -> Option<bool> {
        self.assumes
            .iter()
            .all(|a| self.formula_output(*a))
            .then_some(self.guarantees.iter().all(|g| self.formula_output(*g)))
    }

    pub(crate) fn update(self, event: &Event, state: &[bool], time: Time) -> Self {
        assert!(self.time.0 <= time);
        let new_time = (time, self.time.1 + 1);
        let mut valuations = Vec::with_capacity(self.subformulae.len());
        let mut outputs = Vec::with_capacity(self.subformulae.len());
        for (idx, formula) in self.subformulae.iter().enumerate() {
            match formula {
                ArcPmtl::True => {
                    valuations.push(NumSet::full());
                    outputs.push(NumSet::from_range(self.time, new_time));
                }
                ArcPmtl::False => {
                    valuations.push(NumSet::new());
                    outputs.push(NumSet::new());
                }
                ArcPmtl::Atom(atom) => match atom {
                    Atom::Predicate(p) if state[*p] => {
                        let numset = NumSet::from_range(self.time, new_time);
                        valuations.push(numset.clone());
                        outputs.push(numset);
                    }
                    Atom::Event(e) if event == e => {
                        let numset = NumSet::from_range((new_time.0, new_time.1 - 1), new_time);
                        valuations.push(numset.clone());
                        outputs.push(numset);
                    }
                    _ => {
                        valuations.push(NumSet::new());
                        outputs.push(NumSet::new());
                    }
                },
                ArcPmtl::And(subs) => {
                    let nset =
                        NumSet::intersection(subs.iter().filter_map(|f| outputs.get(f.1)).cloned())
                            .simplify();
                    valuations.push(nset.clone());
                    outputs.push(nset);
                }
                ArcPmtl::Or(subs) => {
                    let nset = subs
                        .iter()
                        .filter_map(|sub| outputs.get(sub.1))
                        .fold(NumSet::new(), |mut union, numset| {
                            union.union(numset);
                            union
                        })
                        .simplify();
                    valuations.push(nset.clone());
                    outputs.push(nset);
                }
                ArcPmtl::Not(subformula) => {
                    let mut nset = outputs.get(subformula.1).expect("nset").clone();
                    nset.complement();
                    nset.cut(self.time, new_time);
                    nset.simplify();
                    valuations.push(nset.clone());
                    outputs.push(nset);
                }
                ArcPmtl::Implies(sub_0, sub_1) => {
                    let out_lhs = outputs.get(sub_0.1).expect("output lhs");
                    let out_rhs = outputs.get(sub_1.1).expect("output rhs");
                    let mut nset = out_lhs.clone();
                    nset.complement();
                    nset.union(out_rhs);
                    nset.cut(self.time, new_time);
                    nset.simplify();
                    valuations.push(nset.clone());
                    outputs.push(nset);
                }
                ArcPmtl::Historically(sub, lower_bound, upper_bound) => {
                    let mut output_sub = outputs.get(sub.1).expect("nset").clone();
                    output_sub.insert_bound(new_time);
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
                    nset_output.cut(self.time, new_time);
                    valuation.cut(self.time, (Time::MAX, Time::MAX));
                    valuations.push(valuation.simplify());
                    outputs.push(nset_output.simplify());
                }
                ArcPmtl::Previously(sub, lower_bound, upper_bound) => {
                    let mut output_sub = outputs.get(sub.1).expect("nset").clone();
                    output_sub.insert_bound(new_time);
                    let mut valuation = self.valuations.get(idx).expect("formula").clone();
                    let mut partial_lower_bound = self.time;
                    let mut output = NumSet::new();
                    for (partial_upper_bound, out_sub) in
                        output_sub.bounds().iter().filter(|(ub, _)| self.time < *ub)
                    {
                        assert!(partial_lower_bound < *partial_upper_bound);
                        assert!(self.time < *partial_upper_bound);
                        assert!(*partial_upper_bound <= new_time);
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
                    valuations.push(valuation.simplify());
                    outputs.push(output.simplify());
                }
                ArcPmtl::Since(sub_0, sub_1, lower_bound, upper_bound) => {
                    let mut nset_lhs = outputs.get(sub_0.1).expect("nset").clone();
                    let nset_rhs_orig = outputs.get(sub_1.1).expect("nset");
                    let mut nset_rhs = nset_rhs_orig.clone();
                    nset_rhs.insert_bound(new_time);
                    nset_lhs.insert_bound(new_time);
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
                    valuations.push(valuation.simplify());
                    outputs.push(nset_output.simplify());
                }
            }
        }
        Self {
            time: new_time,
            assumes: self.assumes,
            guarantees: self.guarantees,
            subformulae: self.subformulae,
            valuations,
            outputs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel_system::ChannelSystemBuilder;

    #[test]
    fn subformulae_1() {
        let subformulae: Vec<ArcPmtl<usize>> = subformulae(
            Pmtl::Since(Box::new((Pmtl::True, Pmtl::True)), 0, Time::MAX).set_subformulae(),
        );
        assert_eq!(
            subformulae,
            vec![
                ArcPmtl::True,
                ArcPmtl::Since(
                    (Arc::new(ArcPmtl::True), 0),
                    (Arc::new(ArcPmtl::True), 0),
                    0,
                    Time::MAX
                ),
            ]
        );
    }

    #[test]
    fn subformulae_2() {
        let subformulae: Vec<ArcPmtl<usize>> = subformulae(
            Pmtl::Since(Box::new((Pmtl::Atom(0), Pmtl::Atom(0))), 0, Time::MAX).set_subformulae(),
        );
        assert_eq!(
            subformulae,
            vec![
                ArcPmtl::Atom(0),
                ArcPmtl::Since(
                    (Arc::new(ArcPmtl::Atom(0)), 0),
                    (Arc::new(ArcPmtl::Atom(0)), 0),
                    0,
                    Time::MAX
                ),
            ]
        );
    }

    #[test]
    fn subformulae_3() {
        let subformulae: Vec<ArcPmtl<usize>> = subformulae(
            Pmtl::And(vec![
                Pmtl::Atom(0),
                Pmtl::Not(Box::new(Pmtl::Atom(0))),
                Pmtl::True,
            ])
            .set_subformulae(),
        );
        assert_eq!(subformulae.len(), 4);
        assert!(matches!(subformulae[0], ArcPmtl::Atom(0) | ArcPmtl::True));
        assert!(matches!(subformulae[1], ArcPmtl::Atom(0) | ArcPmtl::True));
        assert!(matches!(subformulae[2], ArcPmtl::Not((_, 0 | 1))));
    }

    fn dummy_event() -> Event {
        let mut cs = ChannelSystemBuilder::new();
        Event {
            pg_id: cs.new_program_graph(),
            channel: cs.new_channel(crate::Type::Boolean, None),
            event_type: crate::channel_system::EventType::ProbeEmptyQueue,
        }
    }

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
        let dummy_event = dummy_event();
        let mut state = PmtlOracle::new(&[], &[formula]);
        state = state.update(&dummy_event, &[false, true], 0);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[false, true], 1);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[true, true], 2);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true, true], 3);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true, false], 4);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[false, false], 5);
        assert!(!state.output().unwrap());
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
        let mut state = PmtlOracle::new(&[], &[formula]);
        let dummy_event = dummy_event();
        state = state.update(&dummy_event, &[false, true], 0);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[false, true], 1);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[true, true], 2);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true, false], 3);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true, false], 4);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true, false], 5);
        assert!(!state.output().unwrap());
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
        let mut state = PmtlOracle::new(&[], &[formula]);
        let dummy_event = dummy_event();
        state = state.update(&dummy_event, &[false, true], 0);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[false, true], 1);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[true, true], 2);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true, false], 3);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true, false], 4);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true, false], 5);
        assert!(!state.output().unwrap());
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
        let mut state = PmtlOracle::new(&[], &[formula]);
        let dummy_event = dummy_event();
        state = state.update(&dummy_event, &[false, true], 0);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[false, true], 1);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[false, true], 2);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[true, true], 2);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[true, false], 3);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true, false], 4);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true, false], 5);
        assert!(!state.output().unwrap());
    }

    #[test]
    fn historically() {
        let formula = Pmtl::Historically(Box::new(Pmtl::Atom(Atom::Predicate(0))), 1, 2);
        let mut state = PmtlOracle::new(&[], &[formula]);
        let dummy_event = dummy_event();
        state = state.update(&dummy_event, &[false], 0);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[false], 0);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true], 1);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[true], 2);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[true], 3);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[false], 3);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true], 4);
        assert!(!state.output().unwrap());
    }

    #[test]
    fn previously() {
        let formula = Pmtl::Once(Box::new(Pmtl::Atom(Atom::Predicate(0))), 1, 2);
        let mut state = PmtlOracle::new(&[], &[formula]);
        let dummy_event = dummy_event();
        state = state.update(&dummy_event, &[false], 0);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[false], 0);
        assert!(!state.output().unwrap());
        state = state.update(&dummy_event, &[true], 1);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[false], 2);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[false], 3);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[false], 3);
        assert!(state.output().unwrap());
        state = state.update(&dummy_event, &[true], 4);
        assert!(state.output().unwrap());
    }
}
