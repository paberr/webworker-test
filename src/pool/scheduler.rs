use std::cell::Cell;

use super::WebWorkerPool;

pub enum Strategy {
    RoundRobin,
    LoadBased,
}

pub(super) struct Scheduler {
    strategy: Strategy,
    current_worker: Cell<usize>,
}

impl Scheduler {
    pub(super) fn new(strategy: Strategy) -> Self {
        todo!()
    }

    pub(super) fn schedule(&mut self, pool: &WebWorkerPool) -> usize {
        0
    }
}
