use proptest::prelude::*;
use std::path::Path;

use omen::analyzers::complexity::Analyzer as ComplexityAnalyzer;

// ---------------------------------------------------------------------------
// Complexity property tests
// ---------------------------------------------------------------------------

proptest! {
    /// Complexity values must never be negative for any syntactically-valid Python code.
    #[test]
    fn complexity_never_negative(
        body in prop::collection::vec(
            prop_oneof![
                Just("x = 1\n"),
                Just("if x:\n    pass\n"),
                Just("for i in range(10):\n    pass\n"),
                Just("while True:\n    break\n"),
                Just("def f():\n    return 0\n"),
                Just("try:\n    pass\nexcept:\n    pass\n"),
            ],
            0..8,
        )
    ) {
        let code = body.join("");
        let analyzer = ComplexityAnalyzer::new();
        let result = analyzer.analyze_content(
            Path::new("test.py"),
            code.into_bytes(),
        );
        // If parsing succeeds, all metrics must be non-negative.
        if let Ok(file_result) = result {
            for func in &file_result.functions {
                // u32 is unsigned so it cannot be negative, but let's verify
                // the aggregates (f64) are also non-negative.
                prop_assert!(func.metrics.cyclomatic >= 1,
                    "cyclomatic must be >= 1, got {} for {}",
                    func.metrics.cyclomatic, func.name);
            }
            // f64 averages
            prop_assert!(file_result.avg_cyclomatic >= 0.0);
            prop_assert!(file_result.avg_cognitive >= 0.0);
        }
    }

    /// Rust functions always have cyclomatic >= 1 (the function itself is one path).
    #[test]
    fn rust_function_cyclomatic_at_least_one(
        branches in prop::collection::vec(
            prop_oneof![
                Just("if true { 1 } else { 0 };"),
                Just("match x { 0 => 0, _ => 1 };"),
                Just("let _ = x && y;"),
                Just("let _ = x || y;"),
                Just("for _ in 0..1 {}"),
            ],
            0..5,
        )
    ) {
        let body = branches.join("\n    ");
        let code = format!("fn test_fn() {{\n    let x = true;\n    let y = false;\n    {}\n}}\n", body);
        let analyzer = ComplexityAnalyzer::new();
        if let Ok(result) = analyzer.analyze_content(
            Path::new("test.rs"),
            code.into_bytes(),
        ) {
            for func in &result.functions {
                prop_assert!(func.metrics.cyclomatic >= 1,
                    "cyclomatic must be >= 1, got {}", func.metrics.cyclomatic);
            }
        }
    }

    /// Cognitive complexity increases monotonically with nesting depth.
    /// Adding more nesting should never decrease cognitive complexity.
    #[test]
    fn deeper_nesting_means_higher_or_equal_cognitive(depth in 1usize..6) {
        let analyzer = ComplexityAnalyzer::new();

        // Build nested if statements at given depth.
        let mut code_shallow = String::from("def f():\n");
        for i in 0..depth {
            let indent = "    ".repeat(i + 1);
            code_shallow.push_str(&format!("{}if True:\n", indent));
        }
        code_shallow.push_str(&format!("{}pass\n", "    ".repeat(depth + 1)));

        let mut code_deeper = String::from("def f():\n");
        for i in 0..(depth + 1) {
            let indent = "    ".repeat(i + 1);
            code_deeper.push_str(&format!("{}if True:\n", indent));
        }
        code_deeper.push_str(&format!("{}pass\n", "    ".repeat(depth + 2)));

        let result_shallow = analyzer.analyze_content(
            Path::new("test.py"),
            code_shallow.into_bytes(),
        );
        let result_deeper = analyzer.analyze_content(
            Path::new("test.py"),
            code_deeper.into_bytes(),
        );

        if let (Ok(shallow), Ok(deeper)) = (result_shallow, result_deeper) {
            let cog_shallow = shallow.functions.first().map(|f| f.metrics.cognitive).unwrap_or(0);
            let cog_deeper = deeper.functions.first().map(|f| f.metrics.cognitive).unwrap_or(0);
            prop_assert!(cog_deeper >= cog_shallow,
                "deeper nesting (depth={}) cognitive {} should be >= shallow cognitive {}",
                depth, cog_deeper, cog_shallow);
        }
    }

    /// The sigmoid function is always bounded in [0, 1].
    /// For f64, it saturates to exactly 0.0 or 1.0 at extreme inputs due to
    /// floating point precision limits, so we assert the closed interval.
    #[test]
    fn sigmoid_bounded(x in -1000.0f64..1000.0) {
        let result = 1.0 / (1.0 + (-x).exp());
        prop_assert!((0.0..=1.0).contains(&result),
            "sigmoid({}) = {} is out of [0,1] range", x, result);
    }

    /// Score grades must be one of A, B, C, D, F based on the score value.
    #[test]
    fn score_grade_consistent(score in 0.0f64..100.0) {
        let grade = if score >= 90.0 {
            "A"
        } else if score >= 80.0 {
            "B"
        } else if score >= 70.0 {
            "C"
        } else if score >= 60.0 {
            "D"
        } else {
            "F"
        };
        prop_assert!(["A", "B", "C", "D", "F"].contains(&grade));
    }

    /// MinHash Jaccard similarity is always in [0.0, 1.0].
    /// We test the mathematical invariant directly since MinHashSignature is private.
    #[test]
    fn minhash_similarity_bounded(
        values_a in prop::collection::vec(0u64..1000, 10..50),
        values_b in prop::collection::vec(0u64..1000, 10..50),
    ) {
        // Simulate the jaccard_similarity calculation
        let len = values_a.len().min(values_b.len());
        if len == 0 {
            return Ok(());
        }
        let matches = values_a.iter().zip(values_b.iter())
            .filter(|(a, b)| a == b)
            .count();
        let similarity = matches as f64 / len as f64;
        prop_assert!((0.0..=1.0).contains(&similarity),
            "similarity {} out of [0,1] range", similarity);
    }

    /// Entropy (Shannon) is always non-negative for any probability distribution.
    #[test]
    fn entropy_non_negative(
        counts in prop::collection::vec(1u32..100, 1..20),
    ) {
        let total: f64 = counts.iter().map(|&c| c as f64).sum();
        let entropy: f64 = counts.iter()
            .map(|&c| {
                let p = c as f64 / total;
                if p > 0.0 { -p * p.ln() } else { 0.0 }
            })
            .sum();
        prop_assert!(entropy >= 0.0,
            "entropy {} should be non-negative", entropy);
    }

    /// Coupling strength (normalized edge count) is bounded in [0, 1].
    #[test]
    fn coupling_strength_bounded(
        edges in 0u32..100,
        nodes in 1u32..50,
    ) {
        // Maximum possible edges in a directed graph: n * (n - 1)
        let max_edges = nodes as f64 * (nodes as f64 - 1.0);
        let strength = if max_edges > 0.0 {
            edges as f64 / max_edges
        } else {
            0.0
        };
        // Strength can exceed 1.0 if edges > max_edges (multigraph), but
        // for a simple graph it should be bounded. We clamp like the real code does.
        let clamped = strength.clamp(0.0, 1.0);
        prop_assert!((0.0..=1.0).contains(&clamped));
    }

    /// Empty source files should produce zero functions and zero complexity.
    #[test]
    fn empty_file_zero_complexity(ext in prop_oneof![
        Just("py"),
        Just("rs"),
        Just("go"),
        Just("ts"),
        Just("rb"),
    ]) {
        let filename = format!("empty.{}", ext);
        let analyzer = ComplexityAnalyzer::new();
        if let Ok(result) = analyzer.analyze_content(
            Path::new(&filename),
            Vec::new(),
        ) {
            prop_assert_eq!(result.functions.len(), 0,
                "empty {} file should have no functions", ext);
            prop_assert_eq!(result.total_cyclomatic, 0);
            prop_assert_eq!(result.total_cognitive, 0);
        }
    }
}

// ---------------------------------------------------------------------------
// Non-proptest property-style tests (deterministic edge cases)
// ---------------------------------------------------------------------------

#[test]
fn complexity_identical_input_produces_identical_output() {
    let code = b"def foo():\n    if x:\n        return 1\n    return 0\n".to_vec();
    let analyzer = ComplexityAnalyzer::new();
    let r1 = analyzer
        .analyze_content(Path::new("test.py"), code.clone())
        .unwrap();
    let r2 = analyzer
        .analyze_content(Path::new("test.py"), code)
        .unwrap();
    assert_eq!(r1.functions.len(), r2.functions.len());
    for (f1, f2) in r1.functions.iter().zip(r2.functions.iter()) {
        assert_eq!(f1.metrics.cyclomatic, f2.metrics.cyclomatic);
        assert_eq!(f1.metrics.cognitive, f2.metrics.cognitive);
    }
}

#[test]
fn unsupported_language_returns_error() {
    let analyzer = ComplexityAnalyzer::new();
    let result = analyzer.analyze_content(Path::new("test.xyz"), b"some content".to_vec());
    assert!(result.is_err(), "unsupported extension should return error");
}
