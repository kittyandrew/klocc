use std::collections::VecDeque;

#[derive(Debug)]
pub struct RuntimeGraph {
    pub adjacency: Vec<Vec<usize>>,
    pub reverse_ref_count: Vec<usize>,
}

pub fn build(node_count: usize, edges: &[(usize, usize)]) -> RuntimeGraph {
    let mut adjacency = vec![Vec::new(); node_count];
    let mut reverse_ref_count = vec![0usize; node_count];

    for (from, to) in edges {
        adjacency[*from].push(*to);
        reverse_ref_count[*to] += 1;
    }

    RuntimeGraph {
        adjacency,
        reverse_ref_count,
    }
}

pub fn reachable_from(start: usize, adjacency: &[Vec<usize>]) -> Vec<usize> {
    let mut visited = vec![false; adjacency.len()];
    let mut queue = VecDeque::new();
    let mut reachable = Vec::new();

    visited[start] = true;
    queue.push_back(start);

    while let Some(current) = queue.pop_front() {
        reachable.push(current);
        for next in &adjacency[current] {
            if !visited[*next] {
                visited[*next] = true;
                queue.push_back(*next);
            }
        }
    }

    reachable
}

pub fn owner_roots(root_index: usize, adjacency: &[Vec<usize>]) -> Vec<usize> {
    let mut roots = Vec::with_capacity(adjacency[root_index].len() + 1);
    roots.push(root_index);
    for target in &adjacency[root_index] {
        if *target != root_index && !roots.contains(target) {
            roots.push(*target);
        }
    }
    roots
}
