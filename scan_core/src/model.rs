use crate::channel_system::{Channel, ChannelSystem, Event, EventType};
use crate::{
    adaptive_bound, Expression, FnExpression, Pmtl, PmtlOracle, RunOutcome, Time, Tracer, Val,
};
use log::{info, trace};
use rayon::prelude::*;
use std::collections::{btree_map, BTreeMap};
use std::sync::{Arc, Mutex};

/// An atomic variable for [`Pmtl`] formulae.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Atom {
    /// A predicate.
    State(Channel),
    /// An event.
    Event(Event),
}

/// A builder type for [`CsModel`].
#[derive(Debug)]
pub struct CsModelBuilder {
    cs: ChannelSystem,
    ports: BTreeMap<Channel, Val>,
    predicates: Vec<FnExpression<Atom>>,
    assumes: Vec<Pmtl<usize>>,
    guarantees: Vec<Pmtl<usize>>,
}

impl CsModelBuilder {
    /// Creates new [`CsModelBuilder`] from a [`ChannelSystem`].
    pub fn new(initial_state: ChannelSystem) -> Self {
        // TODO: Check predicates are Boolean expressions and that conversion does not fail
        Self {
            cs: initial_state,
            ports: BTreeMap::new(),
            predicates: Vec::new(),
            assumes: Vec::new(),
            guarantees: Vec::new(),
        }
    }

    /// Adds a new port to the [`CsModelBuilder`],
    /// which is given by an [`Channel`] and a default [`Val`] value.
    pub fn add_port(&mut self, channel: Channel, default: Val) {
        // TODO FIXME: error handling and type checking.
        if let btree_map::Entry::Vacant(e) = self.ports.entry(channel) {
            e.insert(default);
        } else {
            panic!("entry is already taken");
        }
    }

    /// Adds a new predicate to the [`CsModelBuilder`],
    /// which is an expression over the CS's channels.
    pub fn add_predicate(&mut self, predicate: Expression<Atom>) -> usize {
        let predicate = FnExpression::<Atom>::from(predicate);
        let _ = predicate.eval(&|port| match port {
            Atom::State(channel) => self.ports.get(&channel).unwrap().clone(),
            Atom::Event(_event) => Val::Boolean(false),
        });
        self.predicates.push(predicate);
        self.predicates.len() - 1
    }

    /// Adds an assume [`Pmtl`] formula to the [`CsModelBuilder`].
    pub fn add_assume(&mut self, assume: Pmtl<usize>) {
        self.assumes.push(assume);
    }

    /// Adds a guarantee [`Pmtl`] formula to the [`CsModelBuilder`].
    pub fn add_guarantee(&mut self, guarantee: Pmtl<usize>) {
        self.guarantees.push(guarantee);
    }

    /// Creates a new [`CsModel`] with the given underlying [`ChannelSystem`] and set of predicates.
    ///
    /// Predicates have to be passed all at once,
    /// as it is not possible to add any further ones after the [`CsModel`] has been initialized.
    pub fn build(self) -> CsModel {
        let mut run_state = RunStatus::default();
        let guarantees = self.guarantees.to_vec();
        run_state.guarantees = vec![0; self.guarantees.len()];
        run_state.running = true;

        CsModel {
            cs: self.cs,
            ports: self.ports,
            predicates: Arc::new(self.predicates),
            oracle: PmtlOracle::new(&self.assumes, &guarantees),
            run_status: Arc::new(Mutex::new(run_state)),
        }
    }
}

/// Represents the state of the current verification run.
#[derive(Debug, Clone, Default)]
pub struct RunStatus {
    /// Whether the verification is still running (`true`) or has terminated (`false`).
    running: bool,
    /// How many runs have succeeded.
    successes: u32,
    /// How many runs have failed.
    failures: u32,
    /// How many times each guarantee has been violated.
    guarantees: Vec<u32>,
}

impl RunStatus {
    /// Gets the running status of the verification run.
    pub fn running(&self) -> bool {
        self.running
    }

    /// Gets the current number of successful runs.
    pub fn successes(&self) -> u32 {
        self.successes
    }

    /// Gets the current number of failed runs.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// Gets the current number of violations for the guarantee corresponding to `index`.
    pub fn guarantee(&self, index: usize) -> Option<u32> {
        self.guarantees.get(index).copied()
    }
}

/// Transition system model based on a [`ChannelSystem`].
///
/// It is essentially a CS which keeps track of the [`Event`]s produced by the execution
/// and determining a set of predicates.
#[derive(Debug, Clone)]
pub struct CsModel {
    cs: ChannelSystem,
    ports: BTreeMap<Channel, Val>,
    predicates: Arc<Vec<FnExpression<Atom>>>,
    oracle: PmtlOracle,
    run_status: Arc<Mutex<RunStatus>>,
}

impl CsModel {
    /// Gets the underlying [`ChannelSystem`].
    #[inline(always)]
    pub fn channel_system(&self) -> &ChannelSystem {
        &self.cs
    }

    /// Gets an `Arc<Mutex<_>>` handle to the state of the current run.
    pub fn run_status(&self) -> Arc<Mutex<RunStatus>> {
        self.run_status.clone()
    }

    fn labels(&self, last_event: &Event) -> Vec<bool> {
        self.predicates
            .iter()
            .map(|prop| {
                if let Val::Boolean(b) = prop.eval(&|port| match port {
                    Atom::State(channel) => self.ports.get(&channel).unwrap().clone(),
                    Atom::Event(event) => Val::Boolean(*last_event == event),
                }) {
                    Some(b)
                } else {
                    None
                }
            })
            .collect::<Option<Vec<_>>>()
            // FIXME: handle error or guarantee it won't happen
            .unwrap()
    }

    #[inline(always)]
    fn time(&self) -> Time {
        self.cs.time()
    }

    /// Statistically verifies [`CsModel`] using adaptive bound and the given parameters.
    /// It allows to optionally pass a [`Tracer`] object to record the produced traces,
    /// and a state [`Mutex`] to be updated with the results as they are produced.
    pub fn par_adaptive<P>(
        &self,
        confidence: f64,
        precision: f64,
        length: usize,
        duration: Time,
        tracer: Option<P>,
    ) where
        P: Tracer<Event>,
    {
        info!("verification starting");
        {
            let run_state = &mut *self.run_status.lock().expect("lock state");
            run_state.successes = 0;
            run_state.failures = 0;
            run_state.running = true;
            run_state.guarantees.iter_mut().for_each(|n| *n = 0);
            // Drop handles!
        }
        // WARN FIXME TODO: Implement algorithm for 2.4 Distributed sample generation in Budde et al.
        (0..usize::MAX)
            .into_par_iter()
            .take_any_while(|_| {
                // .take_while(|_| {
                let mut tracer = tracer.clone();
                if let Some(tracer) = tracer.as_mut() {
                    tracer.init();
                }
                let result = self.clone().experiment(&mut tracer, length, duration);
                let run_status = &mut *self.run_status.lock().expect("lock state");
                if run_status.running {
                    if let Some(tracer) = tracer {
                        tracer.finalize(result);
                    }
                    match result {
                        RunOutcome::Success => {
                            run_status.successes += 1;
                            // If all guarantees are satisfied, the execution is successful
                            info!("runs: {} successes", run_status.successes);
                        }
                        RunOutcome::Fail(guarantee) => {
                            run_status.failures += 1;
                            run_status.guarantees[guarantee] += 1;
                            // If guarantee is violated, we have found a counter-example!
                            info!("runs: {} failures", run_status.failures);
                        }
                        RunOutcome::Incomplete => return true,
                    }
                    let runs = run_status.successes + run_status.failures;
                    // Avoid division by 0
                    let avg = if runs == 0 {
                        0.5f64
                    } else {
                        run_status.successes as f64 / runs as f64
                    };
                    if adaptive_bound(avg, confidence, precision) <= runs as f64 {
                        info!("adaptive bound satisfied");
                        run_status.running = false;
                        false
                    } else {
                        true
                    }
                } else if let Some(tracer) = tracer {
                    tracer.finalize(RunOutcome::Incomplete);
                    false
                } else {
                    false
                }
            })
            .count();
        info!("verification terminating");
    }

    fn experiment<P>(
        mut self,
        tracer: &mut Option<P>,
        max_length: usize,
        duration: Time,
    ) -> RunOutcome
    where
        P: Tracer<Event>,
    {
        use rand::rngs::SmallRng;
        use rand::SeedableRng;

        let mut current_len = 0;
        let rng = &mut SmallRng::from_os_rng();
        trace!("new run starting");
        while let Some(event) = self.cs.montecarlo_execution(rng, duration) {
            // We only need to keep track of events that are associated to the ports
            if let btree_map::Entry::Occupied(mut e) = self.ports.entry(event.channel) {
                if let EventType::Send(ref val) = event.event_type {
                    e.insert(val.clone());
                }
            }
            current_len += 1;
            let state = self.labels(&event);
            let time = self.time();
            if let Some(tracer) = tracer.as_mut() {
                tracer.trace(&event, time, self.ports.values().cloned());
            }
            self.oracle = self.oracle.update(&state, time);
            if self.oracle.output_assumes().is_some() {
                trace!("run undetermined");
                return RunOutcome::Incomplete;
            } else if let Some(i) = self.oracle.output_guarantees() {
                trace!("run fails");
                return RunOutcome::Fail(i);
            } else if current_len >= max_length {
                trace!("run exceeds maximum lenght");
                return RunOutcome::Incomplete;
            } else if !self.run_status.lock().expect("lock state").running {
                trace!("run stopped");
                return RunOutcome::Incomplete;
            }
        }
        trace!("run succeeds");
        RunOutcome::Success
    }
}
