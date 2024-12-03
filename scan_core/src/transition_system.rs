use crate::{Pmtl, PmtlOracle, Time};
use log::{info, trace};
use rand::prelude::*;
use rayon::prelude::*;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Atom<A: Clone + PartialEq + Eq> {
    Predicate(usize),
    Event(A),
}

// WARN: Representing a trace as a Vec of Vecs may be expensive.
pub type Trace<A> = Vec<(Time, A, Vec<bool>)>;

pub trait Publisher<A> {
    fn init(&mut self);

    fn publish(&mut self, action: &A, time: Time, state: &[bool]);

    fn finalize(self, success: Option<bool>);
}

/// Trait implementing a Transition System (TS), as defined in [^1].
/// As such, it is possible to verify it against MTL specifications.
///
/// [^1]: Baier, C., & Katoen, J. (2008). *Principles of model checking*. MIT Press.
pub trait TransitionSystem: Clone + Send + Sync {
    /// The type of the actions that trigger transitions between states in the TS.
    type Action: Debug + Clone + Eq + Send + Sync + Hash;

    /// The label function of the TS valuates the propositions of its set of propositions for the current state.
    // TODO FIXME: bitset instead of Vec<bool>?
    fn labels(&self) -> Vec<bool>;

    // The transition relation relates [`Self::Action`]s and post-states that constitutes possible transitions from the current state.
    // fn transitions(self) -> Vec<(Self::Action, Self)>;

    fn montecarlo_transition<R: Rng>(
        &mut self,
        rng: &mut R,
        max_time: Time,
    ) -> Option<Self::Action>;

    fn time(&self) -> Time {
        0
    }

    fn experiment<P>(
        mut self,
        mut oracle: PmtlOracle<Self::Action>,
        mut publisher: Option<P>,
        length: usize,
        duration: Time,
        run_state: Arc<Mutex<(u32, u32, bool)>>,
    ) -> Option<bool>
    where
        P: Publisher<Self::Action>,
    {
        use rand::rngs::SmallRng;
        use rand::SeedableRng;

        let mut current_len = 0;
        let rng = &mut SmallRng::from_entropy();
        if let Some(publisher) = publisher.as_mut() {
            publisher.init();
        }
        trace!("new run starting");
        while let Some(action) = self.montecarlo_transition(rng, duration) {
            current_len += 1;
            let state = self.labels();
            let time = self.time();
            if let Some(publisher) = publisher.as_mut() {
                publisher.publish(&action, time, &state);
            }
            oracle = oracle.update(&action, &state, time);
            match oracle.output() {
                Some(true) => {
                    if current_len >= length {
                        trace!("run exceeds maximum lenght");
                        if let Some(publisher) = publisher {
                            publisher.finalize(None);
                        }
                        return None;
                    }
                }
                Some(false) => {
                    trace!("run fails");
                    if let Some(publisher) = publisher {
                        publisher.finalize(Some(false));
                    }
                    return Some(false);
                }
                None => {
                    trace!("run undetermined");
                    if let Some(publisher) = publisher {
                        publisher.finalize(None);
                    }
                    return None;
                }
            }
            if !run_state.lock().expect("lock state").2 {
                if let Some(publisher) = publisher {
                    publisher.finalize(None);
                }
                return None;
            }
        }
        trace!("run succeeds");
        if let Some(publisher) = publisher {
            publisher.finalize(Some(true));
        }
        Some(true)
    }

    fn par_adaptive<P>(
        &self,
        guarantees: &[Pmtl<Atom<Self::Action>>],
        assumes: &[Pmtl<Atom<Self::Action>>],
        confidence: f64,
        precision: f64,
        length: usize,
        duration: Time,
        publisher: Option<P>,
        state: Arc<Mutex<(u32, u32, bool)>>,
    ) where
        P: Publisher<Self::Action> + Clone + Send + Sync,
    {
        info!("verification starting");
        let oracle = PmtlOracle::new(assumes, guarantees);
        // WARN FIXME TODO: Implement algorithm for 2.4 Distributed sample generation in Budde et al.
        (0..usize::MAX)
            .into_par_iter()
            .take_any_while(|_| {
                // .take_while(|_| {
                let result = self.clone().experiment(
                    oracle.clone(),
                    publisher.clone(),
                    length,
                    duration,
                    state.clone(),
                );
                let (s, f, running) = &mut *state.lock().expect("lock state");
                if *running {
                    if let Some(result) = result {
                        if result {
                            *s += 1;
                            // If all guarantees are satisfied, the execution is successful
                            info!("runs: {s} successes");
                        } else {
                            *f += 1;
                            // If guarantee is violated, we have found a counter-example!
                            info!("runs: {f} failures");
                        }
                        let n = *s + *f;
                        // Avoid division by 0
                        let avg = if n == 0 { 0.5f64 } else { *s as f64 / n as f64 };
                        if adaptive_bound(avg, confidence, precision) <= n as f64 {
                            info!("adaptive bound satisfied");
                            *running = false;
                        }
                    }
                }
                info!("returning {running} to iter");
                *running
            })
            .count();
        info!("verification terminating");
    }
}

// An efficient statistical model checker for nondeterminism and rare events,
// Carlos E. Budde, Pedro R. D’Argenio, Arnd Hartmanns, Sean Sedwards.
// International Journal on Software Tools for Technology Transfer (2020) 22:759–780
// https://doi.org/10.1007/s10009-020-00563-2

/// Computes Okamoto bound for given confidence and precision.
pub fn okamoto_bound(confidence: f64, precision: f64) -> f64 {
    (2f64 / (1f64 - confidence)).ln() / (2f64 * precision.powf(2f64))
}

/// Computes adaptive bound for given confidence, precision and (partial) experimental results.
pub fn adaptive_bound(avg: f64, confidence: f64, precision: f64) -> f64 {
    4f64 * okamoto_bound(confidence, precision)
        * (0.25f64 - ((avg - 0.5f64).abs() - (2f64 * precision / 3f64)).powf(2f64))
}

/// Computes precision for given experimental results and confidence
/// deriving it from adaptive bound through quadratic equation.
pub fn derive_precision(s: u32, f: u32, confidence: f64) -> f64 {
    let n = s + f;
    let avg = s as f64 / n as f64;
    let k = 2f64 * (2f64 / (1f64 - confidence)).ln();
    // Compute quadratic equation coefficients.
    let a = (n as f64) + (4f64 * k / 9f64);
    let b = -4f64 * k * (avg - 0.5f64).abs() / 3f64;
    let c = k * ((avg - 0.5f64).powf(2f64) - 0.25f64);
    // Take (larger positive) quadratic equation solution.
    (-b + (b.powf(2f64) - 4f64 * a * c).sqrt()) / (2f64 * a)
}
