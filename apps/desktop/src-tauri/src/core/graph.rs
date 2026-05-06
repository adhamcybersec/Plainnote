// SPDX-License-Identifier: AGPL-3.0-or-later
//! Graph data + force-directed layout for the v0.1 graph view (M5).
//!
//! The desktop graph is built from `note_index` (nodes) and `note_link`
//! (edges, only those with a resolved `target_id` — dangling links don't
//! draw). The layout is computed once on demand by Fruchterman-Reingold
//! (1991) and frozen — Sigma.js renders coordinates verbatim.
//!
//! Why Rust-side layout (per plan §3.6 / DECISIONS.md):
//!   - Cap is 5 000 nodes for v0.1; FR converges in ~50 iterations at that
//!     scale and runs in tens of milliseconds in Rust.
//!   - Computing in JS would require shipping a layout library (~50KB)
//!     and pin a frame on the WebView at startup.
//!   - The same algorithm runs in tests at small N for determinism.

use crate::core::index::Index;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Hard cap per plan §6 M5. Above this we surface a "graph too large"
/// message in the UI rather than render a soup.
pub const MAX_GRAPH_NODES: usize = 5_000;

#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub title: String,
    /// Node radius scaled by degree centrality (clamped log-style).
    pub size: f32,
    /// Layout coordinates in arbitrary units; Sigma scales to viewport.
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// True if the vault has more notes than `MAX_GRAPH_NODES`. The
    /// returned subset is the most-connected `MAX_GRAPH_NODES`.
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// `(id, title)` pair for a node, or `(source, target)` for an edge.
type Pair = (String, String);

/// Read nodes (note_index titles) and edges (resolved note_link rows) from
/// the index. Edges with NULL `target_id` (dangling) are excluded.
fn read_graph(index: &Index) -> Result<(Vec<Pair>, Vec<Pair>), GraphError> {
    let conn = index.conn();
    let mut node_stmt =
        conn.prepare("SELECT id, COALESCE(title, '') FROM note_index ORDER BY id")?;
    let nodes: Vec<(String, String)> = node_stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut edge_stmt = conn.prepare(
        "SELECT source, target_id FROM note_link
         WHERE target_id IS NOT NULL",
    )?;
    let edges: Vec<(String, String)> = edge_stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok((nodes, edges))
}

/// Build the full graph payload: read from the index, apply FR layout,
/// scale node sizes by degree.
pub fn build_graph(index: &Index) -> Result<GraphData, GraphError> {
    let (raw_nodes, raw_edges) = read_graph(index)?;
    Ok(build_graph_from_rows(raw_nodes, raw_edges))
}

/// Pure variant — exposed so unit tests can drive the layout deterministically
/// without spinning up a real Index.
pub fn build_graph_from_rows(raw_nodes: Vec<Pair>, raw_edges: Vec<Pair>) -> GraphData {
    let mut truncated = false;

    // Compute degree for sizing + selection-when-over-cap.
    let id_set: std::collections::HashSet<&str> =
        raw_nodes.iter().map(|(id, _)| id.as_str()).collect();
    let mut degree: std::collections::HashMap<String, u32> =
        std::collections::HashMap::with_capacity(raw_nodes.len());
    for (s, t) in &raw_edges {
        if id_set.contains(s.as_str()) && id_set.contains(t.as_str()) {
            *degree.entry(s.clone()).or_insert(0) += 1;
            *degree.entry(t.clone()).or_insert(0) += 1;
        }
    }

    // Selection: if over the cap, keep the top-degree nodes.
    let mut selected: Vec<(String, String)> = if raw_nodes.len() > MAX_GRAPH_NODES {
        truncated = true;
        let mut sorted = raw_nodes.clone();
        sorted.sort_by(|a, b| {
            let da = degree.get(&a.0).copied().unwrap_or(0);
            let db = degree.get(&b.0).copied().unwrap_or(0);
            db.cmp(&da).then(a.0.cmp(&b.0))
        });
        sorted.truncate(MAX_GRAPH_NODES);
        sorted
    } else {
        raw_nodes
    };
    selected.sort_by(|a, b| a.0.cmp(&b.0)); // stable order

    let id_to_idx: std::collections::HashMap<String, usize> = selected
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (id.clone(), i))
        .collect();

    // Filter edges to selected node-set; deduplicate undirected pairs.
    let mut filtered_edges: Vec<(usize, usize)> = Vec::new();
    let mut seen: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    for (s, t) in raw_edges {
        let (Some(&si), Some(&ti)) = (id_to_idx.get(&s), id_to_idx.get(&t)) else {
            continue;
        };
        if si == ti {
            continue;
        }
        let pair = if si < ti { (si, ti) } else { (ti, si) };
        if seen.insert(pair) {
            filtered_edges.push((si, ti));
        }
    }

    let positions = layout_fr(selected.len(), &filtered_edges);

    let nodes: Vec<GraphNode> = selected
        .into_iter()
        .enumerate()
        .map(|(i, (id, title))| {
            let d = degree.get(&id).copied().unwrap_or(0) as f32;
            // Node radius: 4 + log2(degree+1) * 2, clamped.
            let size = (4.0 + (d + 1.0).log2() * 2.0).min(20.0);
            GraphNode {
                id,
                title,
                size,
                x: positions[i].0,
                y: positions[i].1,
            }
        })
        .collect();

    let edges: Vec<GraphEdge> = filtered_edges
        .into_iter()
        .map(|(s, t)| GraphEdge {
            source: nodes[s].id.clone(),
            target: nodes[t].id.clone(),
        })
        .collect();

    GraphData {
        nodes,
        edges,
        truncated,
    }
}

/// Fruchterman-Reingold force-directed layout. Returns `n` (x, y) pairs.
///
/// Algorithm (1991):
///   - Random initial positions in the unit square.
///   - For `iters` iterations:
///       - Repulsive force between every pair of nodes ∝ k² / dist
///       - Attractive force between connected pairs ∝ dist² / k
///       - Limit per-step displacement to a cooling temperature `t` that
///         decays linearly to ~0 across iterations.
///   - After convergence, normalize into [-1, 1] x [-1, 1].
fn layout_fr(n: usize, edges: &[(usize, usize)]) -> Vec<(f32, f32)> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![(0.0, 0.0)];
    }

    // Deterministic RNG: layouts must be reproducible (same vault → same
    // graph), otherwise the user sees a different shape on every reload.
    let mut rng = ChaCha8Rng::seed_from_u64(42);
    let area = 1.0_f32;
    let k = (area / n as f32).sqrt();
    // Iter budget by scale. FR is O(n² · iters); the cooling schedule
    // means later iterations contribute much less. At 5k nodes we cap
    // at 30 iters to keep the layout call under ~1s on a typical CPU.
    let iters = match n {
        0..=64 => 80,
        65..=512 => 100,
        513..=2_048 => 60,
        _ => 30,
    };
    let mut t = (n as f32).sqrt() * 0.1;
    let cooling = t / iters as f32;

    let mut pos: Vec<(f32, f32)> = (0..n)
        .map(|_| (rng.gen_range(-0.5..0.5), rng.gen_range(-0.5..0.5)))
        .collect();
    let mut disp = vec![(0.0_f32, 0.0_f32); n];

    for _ in 0..iters {
        // Reset displacements.
        disp.fill((0.0, 0.0));
        // Repulsive: O(n²) is fine at our 5k cap (≤25M ops, no allocation).
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = pos[i].0 - pos[j].0;
                let dy = pos[i].1 - pos[j].1;
                let dist = (dx * dx + dy * dy).sqrt().max(1e-4);
                let force = k * k / dist;
                let fx = dx / dist * force;
                let fy = dy / dist * force;
                disp[i].0 += fx;
                disp[i].1 += fy;
                disp[j].0 -= fx;
                disp[j].1 -= fy;
            }
        }
        // Attractive forces along edges.
        for &(s, e) in edges {
            let dx = pos[s].0 - pos[e].0;
            let dy = pos[s].1 - pos[e].1;
            let dist = (dx * dx + dy * dy).sqrt().max(1e-4);
            let force = dist * dist / k;
            let fx = dx / dist * force;
            let fy = dy / dist * force;
            disp[s].0 -= fx;
            disp[s].1 -= fy;
            disp[e].0 += fx;
            disp[e].1 += fy;
        }
        // Apply displacements with the cooling cap.
        for i in 0..n {
            let d = (disp[i].0 * disp[i].0 + disp[i].1 * disp[i].1)
                .sqrt()
                .max(1e-4);
            let cap = d.min(t);
            pos[i].0 += disp[i].0 / d * cap;
            pos[i].1 += disp[i].1 / d * cap;
        }
        t = (t - cooling).max(0.0);
    }

    // Normalize into [-1, 1] x [-1, 1] for stable rendering.
    let (mut min_x, mut max_x) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut min_y, mut max_y) = (f32::INFINITY, f32::NEG_INFINITY);
    for &(x, y) in &pos {
        if x < min_x {
            min_x = x;
        }
        if x > max_x {
            max_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if y > max_y {
            max_y = y;
        }
    }
    let range_x = (max_x - min_x).max(1e-4);
    let range_y = (max_y - min_y).max(1e-4);
    for p in &mut pos {
        p.0 = (p.0 - min_x) / range_x * 2.0 - 1.0;
        p.1 = (p.1 - min_y) / range_y * 2.0 - 1.0;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_index_yields_empty_graph() {
        let g = build_graph_from_rows(vec![], vec![]);
        assert!(g.nodes.is_empty());
        assert!(g.edges.is_empty());
        assert!(!g.truncated);
    }

    #[test]
    fn single_node_lands_at_origin() {
        let g = build_graph_from_rows(vec![("a".into(), "A".into())], vec![]);
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].x, 0.0);
        assert_eq!(g.nodes[0].y, 0.0);
    }

    #[test]
    fn dangling_edges_are_dropped() {
        // Edge to "nowhere" — its target isn't in the node set; must not appear.
        let g = build_graph_from_rows(
            vec![("a".into(), "A".into()), ("b".into(), "B".into())],
            vec![("a".into(), "nowhere".into()), ("a".into(), "b".into())],
        );
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].source, "a");
        assert_eq!(g.edges[0].target, "b");
    }

    #[test]
    fn duplicate_edges_are_deduplicated() {
        let g = build_graph_from_rows(
            vec![("a".into(), "A".into()), ("b".into(), "B".into())],
            vec![("a".into(), "b".into()), ("b".into(), "a".into())],
        );
        assert_eq!(g.edges.len(), 1, "got {:?}", g.edges);
    }

    #[test]
    fn self_loops_are_dropped() {
        let g = build_graph_from_rows(
            vec![("a".into(), "A".into())],
            vec![("a".into(), "a".into())],
        );
        assert!(g.edges.is_empty());
    }

    #[test]
    fn node_size_scales_with_degree() {
        // 'a' connects to b, c, d — should be larger than the leaves.
        let g = build_graph_from_rows(
            vec![
                ("a".into(), "A".into()),
                ("b".into(), "B".into()),
                ("c".into(), "C".into()),
                ("d".into(), "D".into()),
            ],
            vec![
                ("a".into(), "b".into()),
                ("a".into(), "c".into()),
                ("a".into(), "d".into()),
            ],
        );
        let a_size = g.nodes.iter().find(|n| n.id == "a").unwrap().size;
        let b_size = g.nodes.iter().find(|n| n.id == "b").unwrap().size;
        assert!(a_size > b_size, "a={a_size} b={b_size}");
    }

    #[test]
    fn layout_is_deterministic_across_runs() {
        // Same input → same coordinates. Critical so the user doesn't see
        // a different graph shape every time they open the view.
        let nodes = vec![
            ("a".into(), "A".into()),
            ("b".into(), "B".into()),
            ("c".into(), "C".into()),
        ];
        let edges = vec![("a".into(), "b".into()), ("b".into(), "c".into())];
        let g1 = build_graph_from_rows(nodes.clone(), edges.clone());
        let g2 = build_graph_from_rows(nodes, edges);
        assert_eq!(g1.nodes, g2.nodes);
        assert_eq!(g1.edges, g2.edges);
    }

    #[test]
    fn coordinates_are_in_normalized_range() {
        let nodes: Vec<(String, String)> = (0..20)
            .map(|i| (format!("n{i}"), format!("Node {i}")))
            .collect();
        let edges: Vec<(String, String)> = (0..19)
            .map(|i| (format!("n{i}"), format!("n{}", i + 1)))
            .collect();
        let g = build_graph_from_rows(nodes, edges);
        for n in &g.nodes {
            assert!(n.x >= -1.0 - 1e-3 && n.x <= 1.0 + 1e-3, "x={}", n.x);
            assert!(n.y >= -1.0 - 1e-3 && n.y <= 1.0 + 1e-3, "y={}", n.y);
        }
    }

    #[test]
    fn truncates_to_top_degree_when_over_cap() {
        // Build a star with one super-hub plus MAX_GRAPH_NODES leaves.
        // The hub must survive truncation; we use a small cap via test
        // configuration would be cleaner but MAX_GRAPH_NODES is a const.
        // This smoke-tests the path: with 5001 nodes and one star, the
        // result should be MAX_GRAPH_NODES nodes and `truncated == true`.
        let mut nodes = vec![("hub".into(), "Hub".into())];
        for i in 0..MAX_GRAPH_NODES {
            nodes.push((format!("n{i}"), format!("Leaf {i}")));
        }
        let edges: Vec<(String, String)> = (0..MAX_GRAPH_NODES)
            .map(|i| ("hub".into(), format!("n{i}")))
            .collect();
        let g = build_graph_from_rows(nodes, edges);
        assert!(g.truncated);
        assert_eq!(g.nodes.len(), MAX_GRAPH_NODES);
        assert!(
            g.nodes.iter().any(|n| n.id == "hub"),
            "hub must survive truncation"
        );
    }
}
