//! Rendering does not own time. This cursor advances explicit, ordered rule events.
use crate::model::{Event, Graph, Plan};

pub struct Timeline {
    pub plan: Plan,
    pub cursor: usize,
    pub tick: u64,
    waiting: u32,
    tail: u32,
}

impl Timeline {
    pub fn new(plan: Plan, tail: u32) -> Self {
        Self {
            plan,
            cursor: 0,
            tick: 0,
            waiting: 0,
            tail,
        }
    }

    pub fn finished(&self) -> bool {
        self.cursor == self.plan.events.len() && self.waiting == 0 && self.tail == 0
    }

    /// Returns whether graph buffers changed and the number of solver ticks to run.
    /// Never combines time across a mutation boundary.
    pub fn next(&mut self, graph: &mut Graph, budget: u32) -> Result<(bool, u32), String> {
        let mut changed = false;
        while self.waiting == 0 && self.cursor < self.plan.events.len() {
            let event = &self.plan.events[self.cursor];
            if let Event::Wait { ticks } = event {
                self.waiting = *ticks;
            } else {
                graph.apply(event)?;
                changed = true;
            }
            self.cursor += 1;
        }
        let ticks = if self.waiting > 0 {
            let count = budget.min(self.waiting);
            self.waiting -= count;
            count
        } else {
            let count = budget.min(self.tail);
            self.tail -= count;
            count
        };
        self.tick += u64::from(ticks);
        Ok((changed, ticks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan(events: Vec<Event>) -> Plan {
        Plan {
            events,
            total_ticks: 5,
            node_count: 0,
            edge_count: 0,
        }
    }
    #[test]
    fn timeline_has_exact_waits_and_tail() {
        let mut time = Timeline::new(
            plan(vec![Event::Wait { ticks: 2 }, Event::Wait { ticks: 3 }]),
            4,
        );
        let mut graph = Graph::new(1);
        let mut chunks = vec![];
        while !time.finished() {
            chunks.push(time.next(&mut graph, 4).unwrap().1);
        }
        assert_eq!(chunks, vec![2, 3, 4]);
        assert_eq!(time.tick, 9);
    }
    #[test]
    fn zero_wait_does_not_hang() {
        let mut time = Timeline::new(plan(vec![Event::Wait { ticks: 0 }]), 0);
        assert_eq!(time.next(&mut Graph::new(1), 4).unwrap(), (false, 0));
        assert!(time.finished());
    }
    #[test]
    fn flushes_mutation_at_exact_sample_boundary() {
        let batch: Event =
            serde_json::from_str(r##"{"op":"batch","nodes":[{"id":"B"}],"edges":[]}"##).unwrap();
        let mut time = Timeline::new(plan(vec![Event::Wait { ticks: 4 }, batch]), 0);
        let mut graph = Graph::new(1);
        assert_eq!(time.next(&mut graph, 4).unwrap(), (false, 4));
        assert_eq!(time.next(&mut graph, 0).unwrap(), (true, 0));
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(time.tick, 4);
        assert!(time.finished());
    }
}
