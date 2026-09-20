use super::store::{StateId, StateStore};
use ac_core::{
    Certificate, DEFAULT_N_GEN, MoveId, Presentation, all_move_ids, apply_move, is_ac_target,
    move_from_id, reduce, replay_certificate,
};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphStats {
    pub source_certificates: usize,
    pub trajectory_states: usize,
    pub unique_states: u32,
    pub original_edges: usize,
    pub target_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Edge {
    destination: StateId,
    move_id: MoveId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosureStats {
    pub tested_moves: u64,
    pub known_destinations: u64,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathStats {
    pub reachable_states: u64,
    pub elapsed: Duration,
}

pub struct TrajectoryGraph {
    store: StateStore,
    roots: HashMap<String, StateId>,
    source_certificates: usize,
    trajectory_states: usize,
    original_edges: Vec<Vec<Edge>>,
    induced_edges: Vec<Vec<Edge>>,
    target_id: StateId,
    next_steps: Vec<Option<Edge>>,
    distances: Vec<Option<u32>>,
}

impl Default for TrajectoryGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl TrajectoryGraph {
    pub fn new() -> Self {
        let mut store = StateStore::new();
        let target_id = store
            .insert_new(&[1], &[2], None, 0)
            .expect("target must be unique in an empty graph");
        Self {
            store,
            roots: HashMap::new(),
            source_certificates: 0,
            trajectory_states: 0,
            original_edges: vec![Vec::new()],
            induced_edges: Vec::new(),
            target_id,
            next_steps: Vec::new(),
            distances: Vec::new(),
        }
    }

    pub fn ingest_certificate(
        &mut self,
        challenge_id: impl Into<String>,
        start: &Presentation,
        certificate: &[MoveId],
    ) -> Result<StateId, String> {
        let challenge_id = challenge_id.into();
        if self.roots.contains_key(&challenge_id) {
            return Err(format!("duplicate_challenge_id: {challenge_id}"));
        }
        let root = Presentation {
            relators: [
                reduce(&start.relators[0], DEFAULT_N_GEN).map_err(|e| e.0)?,
                reduce(&start.relators[1], DEFAULT_N_GEN).map_err(|e| e.0)?,
            ],
        };
        let mut states = Vec::with_capacity(certificate.len() + 1);
        let mut current = root.clone();
        states.push(current.clone());
        for &move_id in certificate {
            let mv = move_from_id(move_id).map_err(|e| e.0)?;
            current = apply_move(&current, &mv, DEFAULT_N_GEN).map_err(|e| e.0)?;
            states.push(current.clone());
        }
        if !is_ac_target(&current) {
            return Err("certificate_does_not_reach_target".into());
        }

        let mut ids = Vec::with_capacity(states.len());
        for state in &states {
            let id = self
                .store
                .insert_new(&state.relators[0], &state.relators[1], None, 0)
                .or_else(|| self.store.find(&state.relators[0], &state.relators[1]))
                .expect("inserted or found state");
            while self.original_edges.len() <= id as usize {
                self.original_edges.push(Vec::new());
            }
            ids.push(id);
        }
        for (window, &move_id) in ids.windows(2).zip(certificate) {
            self.original_edges[window[0] as usize].push(Edge {
                destination: window[1],
                move_id,
            });
        }
        self.source_certificates += 1;
        self.trajectory_states += states.len();
        self.roots.insert(challenge_id, ids[0]);
        Ok(ids[0])
    }

    pub fn root_id(&self, challenge_id: &str) -> Option<StateId> {
        self.roots.get(challenge_id).copied()
    }

    pub fn state(&self, id: StateId) -> Presentation {
        let (mut r0, mut r1) = (Vec::new(), Vec::new());
        self.store.copy_state(id, &mut r0, &mut r1);
        Presentation { relators: [r0, r1] }
    }

    pub fn stats(&self) -> GraphStats {
        GraphStats {
            source_certificates: self.source_certificates,
            trajectory_states: self.trajectory_states,
            unique_states: self.store.len(),
            original_edges: self.original_edges.iter().map(Vec::len).sum(),
            target_id: self.target_id,
        }
    }

    pub fn induce_primitive_edges(&mut self) -> ClosureStats {
        let started = Instant::now();
        let state_count = self.store.len();
        self.induced_edges = vec![Vec::new(); state_count as usize];
        let mut tested_moves = 0;
        let mut known_destinations = 0;
        let (mut r0, mut r1) = (Vec::new(), Vec::new());
        for source in 0..state_count {
            self.store.copy_state(source, &mut r0, &mut r1);
            let state = Presentation {
                relators: [r0.clone(), r1.clone()],
            };
            for move_id in all_move_ids() {
                tested_moves += 1;
                let mv = move_from_id(move_id).expect("official move table is complete");
                let destination = apply_move(&state, &mv, DEFAULT_N_GEN)
                    .expect("official move applies to reduced state");
                if let Some(destination_id) = self
                    .store
                    .find(&destination.relators[0], &destination.relators[1])
                {
                    known_destinations += 1;
                    self.induced_edges[source as usize].push(Edge {
                        destination: destination_id,
                        move_id,
                    });
                }
            }
        }
        ClosureStats {
            tested_moves,
            known_destinations,
            elapsed: started.elapsed(),
        }
    }

    pub fn reverse_shortest_paths(&mut self) -> PathStats {
        let started = Instant::now();
        let state_count = self.store.len() as usize;
        let mut predecessors = vec![Vec::new(); state_count];
        for (source, edges) in self.induced_edges.iter().enumerate() {
            for &edge in edges {
                predecessors[edge.destination as usize].push(Edge {
                    destination: source as StateId,
                    move_id: edge.move_id,
                });
            }
        }
        self.distances = vec![None; state_count];
        self.next_steps = vec![None; state_count];
        self.distances[self.target_id as usize] = Some(0);
        let mut queue = VecDeque::from([self.target_id]);
        while let Some(destination) = queue.pop_front() {
            let destination_distance = self.distances[destination as usize].unwrap();
            for predecessor in predecessors[destination as usize].iter().copied() {
                let source = predecessor.destination;
                if self.distances[source as usize].is_none() {
                    self.distances[source as usize] = Some(destination_distance + 1);
                    self.next_steps[source as usize] = Some(Edge {
                        destination,
                        move_id: predecessor.move_id,
                    });
                    queue.push_back(source);
                }
            }
        }
        PathStats {
            reachable_states: self.distances.iter().flatten().count() as u64,
            elapsed: started.elapsed(),
        }
    }

    pub fn shortest_certificate(&self, root: StateId) -> Option<Certificate> {
        self.distances.get(root as usize)?.as_ref()?;
        let mut current = root;
        let mut certificate = Vec::new();
        while current != self.target_id {
            let edge = self.next_steps[current as usize]?;
            certificate.push(edge.move_id);
            current = edge.destination;
        }
        let start = self.state(root);
        replay_certificate(&start, &certificate)
            .ok()
            .map(|_| certificate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ac_core::{is_ac_target, replay_certificate};

    fn pres(r0: &[i8], r1: &[i8]) -> Presentation {
        Presentation {
            relators: [r0.to_vec(), r1.to_vec()],
        }
    }

    #[test]
    fn ingest_deduplicates_exact_states_and_keeps_order() {
        let mut graph = TrajectoryGraph::new();
        let root = graph
            .ingest_certificate("one", &pres(&[1, 2], &[2]), &[3])
            .unwrap();
        let second = graph
            .ingest_certificate("two", &pres(&[-2, -1], &[2]), &[0, 3])
            .unwrap();

        assert_ne!(root, second);
        let stats = graph.stats();
        assert_eq!(stats.source_certificates, 2);
        assert_eq!(stats.trajectory_states, 5);
        assert_eq!(stats.unique_states, 3);
        assert_eq!(stats.original_edges, 3);
        assert_eq!(stats.target_id, 0);
        assert_eq!(graph.root_id("one"), Some(root));
        assert_eq!(graph.root_id("two"), Some(second));
        assert_eq!(graph.state(root), pres(&[1, 2], &[2]));
    }

    #[test]
    fn induced_closure_reconstructs_a_shortest_known_path() {
        let mut graph = TrajectoryGraph::new();
        let root = graph
            .ingest_certificate("one", &pres(&[1, 2], &[2]), &[3])
            .unwrap();

        let closure = graph.induce_primitive_edges();
        assert_eq!(
            closure.tested_moves,
            graph.stats().unique_states as u64 * 14
        );
        let paths = graph.reverse_shortest_paths();
        assert_eq!(paths.reachable_states, graph.stats().unique_states as u64);
        let cert = graph.shortest_certificate(root).unwrap();
        assert_eq!(cert, vec![3]);
        assert!(is_ac_target(
            &replay_certificate(&pres(&[1, 2], &[2]), &cert).unwrap()
        ));
    }
}
