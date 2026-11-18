use super::*;

#[test]
fn test_calculate_coupling_score_uses_hub_concentration() {
    // A repo with low avg degree but extreme hub nodes should score lower
    // than one with uniform low degree
    let uniform = crate::analyzers::graph::Analysis {
        nodes: (0..100)
            .map(|i| crate::analyzers::graph::Node {
                file: format!("file{i}.rs"),
                pagerank: 0.01,
                betweenness: 0.01,
                in_degree: 1,
                out_degree: 1,
                instability: 0.5,
            })
            .collect(),
        edges: vec![],
        cycles: vec![],
        summary: crate::analyzers::graph::AnalysisSummary {
            total_nodes: 100,
            total_edges: 100,
            avg_degree: 2.0,
            cycle_count: 0,
        },
    };
    let hub_heavy = crate::analyzers::graph::Analysis {
        nodes: {
            let mut nodes: Vec<_> = (0..100)
                .map(|i| crate::analyzers::graph::Node {
                    file: format!("file{i}.rs"),
                    pagerank: 0.01,
                    betweenness: 0.01,
                    in_degree: 0,
                    out_degree: 0,
                    instability: 0.5,
                })
                .collect();
            // One mega-hub with 141 connections
            nodes[0].in_degree = 70;
            nodes[0].out_degree = 71;
            nodes
        },
        edges: vec![],
        cycles: vec![],
        summary: crate::analyzers::graph::AnalysisSummary {
            total_nodes: 100,
            total_edges: 141,
            avg_degree: 1.41, // low avg because most nodes are 0
            cycle_count: 0,
        },
    };
    let uniform_score = calculate_coupling_score(&uniform);
    let hub_score = calculate_coupling_score(&hub_heavy);
    assert!(
        uniform_score > hub_score,
        "uniform coupling should score higher than hub-heavy: uniform={uniform_score}, hub={hub_score}"
    );
}

#[test]
fn test_calculate_coupling_score_not_always_near_100() {
    // A repo with 4835 nodes, 1 cycle, avg_degree 1.2, but extreme hubs
    // should not just score 95
    let result = crate::analyzers::graph::Analysis {
        nodes: {
            let mut nodes: Vec<_> = (0..4835)
                .map(|i| crate::analyzers::graph::Node {
                    file: format!("file{i}.rs"),
                    pagerank: 0.0002,
                    betweenness: 0.0,
                    in_degree: 0,
                    out_degree: 0,
                    instability: 0.5,
                })
                .collect();
            // Top hubs from real data
            nodes[0].in_degree = 70;
            nodes[0].out_degree = 71;
            nodes[1].in_degree = 57;
            nodes[1].out_degree = 57;
            nodes[2].in_degree = 50;
            nodes[2].out_degree = 55;
            nodes
        },
        edges: vec![],
        cycles: vec![vec!["a".into(), "b".into()]],
        summary: crate::analyzers::graph::AnalysisSummary {
            total_nodes: 4835,
            total_edges: 2907,
            avg_degree: 1.2,
            cycle_count: 1,
        },
    };
    let score = calculate_coupling_score(&result);
    // Should reflect the hub concentration, not just avg_degree
    assert!(
        score < 90.0,
        "hub-heavy graph should not score 95, got {score}"
    );
}
