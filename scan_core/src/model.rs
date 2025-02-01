use crate::channel_system::{Channel, ChannelSystem, Event, EventType};
use crate::{Expression, FnExpression, Pmtl, PmtlOracle, Time, Tracer, Val};
use log::{info, trace};
use rayon::prelude::*;
use std::collections::HashMap;
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
    ports: HashMap<Channel, Val>,
    predicates: Vec<FnExpression<Atom>>,
    assumes: Vec<Pmtl<usize>>,
    guarantees: Vec<(String, Pmtl<usize>)>,
}

impl CsModelBuilder {
    /// Creates new [`CsModelBuilder`] from a [`ChannelSystem`].
    pub fn new(initial_state: ChannelSystem) -> Self {
        // TODO: Check predicates are Boolean expressions and that conversion does not fail
        Self {
            cs: initial_state,
            ports: HashMap::new(),
            predicates: Vec::new(),
            assumes: Vec::new(),
            guarantees: Vec::new(),
        }
    }

    /// Adds a new port to the [`CsModelBuilder`],
    /// which is given by an [`Channel`] and a default [`Val`] value.
    pub fn add_port(&mut self, channel: Channel, default: Val) {
        // TODO FIXME: error handling and type checking.
        if let std::collections::hash_map::Entry::Vacant(e) = self.ports.entry(channel) {
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

    /// Adds an guarantee [`Pmtl`] formula to the [`CsModelBuilder`].
    pub fn add_guarantee(&mut self, name: String, guarantee: Pmtl<usize>) {
        self.guarantees.push((name, guarantee));
    }

    /// Creates a new [`CsModel`] with the given underlying [`ChannelSystem`] and set of predicates.
    ///
    /// Predicates have to be passed all at once,
    /// as it is not possible to add any further ones after the [`CsModel`] has been initialized.
    pub fn build(self) -> CsModel {
        let mut run_state = RunStatus::default();
        let guarantees = self
            .guarantees
            .iter()
            .map(|(_, g)| g)
            .cloned()
            .collect::<Vec<_>>();
        run_state.guarantees = self.guarantees.into_iter().map(|(s, _)| (s, 0)).collect();

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
    pub successes: u32,
    pub failures: u32,
    pub running: bool,
    pub guarantees: Vec<(String, u32)>,
}

/// Transition system model based on a [`ChannelSystem`].
///
/// It is essentially a CS which keeps track of the [`Event`]s produced by the execution
/// and determining a set of predicates.
#[derive(Debug, Clone)]
pub struct CsModel {
    cs: ChannelSystem,
    ports: HashMap<Channel, Val>,
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
        P: Tracer<Event> + Clone + Send + Sync,
    {
        info!("verification starting");
        {
            let run_state = &mut *self.run_status.lock().expect("lock state");
            run_state.successes = 0;
            run_state.failures = 0;
            run_state.running = true;
            run_state.guarantees.iter_mut().for_each(|(_, n)| *n = 0);
            // Drop handles!
        }
        // WARN FIXME TODO: Implement algorithm for 2.4 Distributed sample generation in Budde et al.
        (0..usize::MAX)
            .into_par_iter()
            .take_any_while(|_| {
                // .take_while(|_| {
                let result = self.clone().experiment(tracer.clone(), length, duration);
                let run_status = &mut *self.run_status.lock().expect("lock state");
                if run_status.running {
                    if let (Some(result), guarantee) = result {
                        if result {
                            run_status.successes += 1;
                            // If all guarantees are satisfied, the execution is successful
                            info!("runs: {} successes", run_status.successes);
                        } else {
                            run_status.failures += 1;
                            run_status.guarantees[guarantee.unwrap()].1 += 1;
                            // If guarantee is violated, we have found a counter-example!
                            info!("runs: {} failures", run_status.failures);
                        }
                        let n = run_status.successes + run_status.failures;
                        // Avoid division by 0
                        let avg = if n == 0 {
                            0.5f64
                        } else {
                            run_status.successes as f64 / n as f64
                        };
                        if crate::adaptive_bound(avg, confidence, precision) <= n as f64 {
                            info!("adaptive bound satisfied");
                            run_status.running = false;
                        }
                    }
                }
                info!("returning {} to iter", run_status.running);
                run_status.running
            })
            .count();
        info!("verification terminating");
    }

    fn experiment<P>(
        mut self,
        mut tracer: Option<P>,
        max_length: usize,
        duration: Time,
    ) -> (Option<bool>, Option<usize>)
    where
        P: Tracer<Event>,
    {
        use rand::rngs::SmallRng;
        use rand::SeedableRng;

        let mut current_len = 0;
        let rng = &mut SmallRng::from_os_rng();
        if let Some(publisher) = tracer.as_mut() {
            publisher.init();
        }
        trace!("new run starting");
        while let Some(event) = self.cs.montecarlo_execution(rng, duration) {
            if let EventType::Send(ref val) = event.event_type {
                self.ports.insert(event.channel, val.clone());
            }
            current_len += 1;
            let state = self.labels(&event);
            let time = self.time();
            if let Some(tracer) = tracer.as_mut() {
                tracer.trace(&event, time, &state);
            }
            self.oracle = self.oracle.update(&state, time);
            if let Some(i) = self.oracle.output_assumes() {
                trace!("run undetermined");
                if let Some(publisher) = tracer {
                    publisher.finalize(None);
                }
                return (None, Some(i));
            } else if let Some(i) = self.oracle.output_guarantees() {
                trace!("run fails");
                if let Some(tracer) = tracer {
                    tracer.finalize(Some(false));
                }
                return (Some(false), Some(i));
            } else if current_len >= max_length {
                trace!("run exceeds maximum lenght");
                if let Some(tracer) = tracer {
                    tracer.finalize(None);
                }
                return (None, None);
            } else if !self.run_status.lock().expect("lock state").running {
                trace!("run stopped");
                if let Some(tracer) = tracer {
                    tracer.finalize(None);
                }
                return (None, None);
            }
        }
        trace!("run succeeds");
        if let Some(tracer) = tracer {
            tracer.finalize(Some(true));
        }
        (Some(true), None)
    }
}
