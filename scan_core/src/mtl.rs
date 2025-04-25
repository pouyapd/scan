use crate::{Oracle, Time};

#[derive(Clone)]
pub enum Mtl<V: Clone> {
    Atom(V),
    Until(V, V),
}

#[derive(Default, Clone)]
pub struct MtlOracle {
    assumes: Vec<(Mtl<usize>, Option<bool>)>,
    guarantees: Vec<(Mtl<usize>, Option<bool>)>,
}

impl MtlOracle {
    pub fn add_guarantee(&mut self, mtl: Mtl<usize>) {
        self.guarantees.push((mtl, None));
    }

    pub fn add_assume(&mut self, mtl: Mtl<usize>) {
        self.guarantees.push((mtl, None));
    }
}

impl Oracle for MtlOracle {
    fn update(&mut self, state: &[bool], time: Time) {
        self.guarantees
            .iter_mut()
            .chain(self.assumes.iter_mut())
            .for_each(|(mtl, opt)| {
                *opt = match mtl {
                    Mtl::Atom(i) => state.get(*i).cloned(),
                    Mtl::Until(lhs, rhs) => match opt {
                        Some(_) => *opt,
                        None => {
                            if state.get(*rhs).is_some_and(|b| *b) {
                                Some(true)
                            } else if state.get(*lhs).is_some_and(|b| *b) {
                                None
                            } else {
                                Some(false)
                            }
                        }
                    },
                }
            })
    }

    fn output_assumes(&self) -> Option<usize> {
        self.assumes
            .iter()
            .enumerate()
            .find_map(|(i, (_, opt))| opt.is_some_and(|b| !b).then_some(i))
    }

    fn output_guarantees(&self) -> Option<usize> {
        self.guarantees
            .iter()
            .enumerate()
            .find_map(|(i, (_, opt))| opt.is_some_and(|b| !b).then_some(i))
    }

    fn final_output_assumes(&self) -> Option<usize> {
        self.assumes
            .iter()
            .enumerate()
            .find_map(|(i, (_, opt))| (!opt.is_some_and(|b| b)).then_some(i))
    }

    fn final_output_guarantees(&self) -> Option<usize> {
        self.guarantees
            .iter()
            .enumerate()
            .find_map(|(i, (_, opt))| (!opt.is_some_and(|b| b)).then_some(i))
    }
}
