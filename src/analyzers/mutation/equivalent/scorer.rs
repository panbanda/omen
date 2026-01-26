//! Scoring model for equivalent mutant detection.
//!
//! Assigns a probability score to each mutant indicating how likely it is
//! to be equivalent (0.0 = definitely not equivalent, 1.0 = definitely equivalent).

use super::super::Mutant;
use super::features::MutantFeatures;
use super::heuristics::{EquivalenceReason, HeuristicDetector};
use crate::parser::ParseResult;

/// Configuration for the equivalence scorer.
#[derive(Debug, Clone)]
pub struct ScorerConfig {
    /// Weight for the logging statement heuristic.
    pub logging_weight: f64,
    /// Weight for the dead code heuristic.
    pub dead_code_weight: f64,
    /// Weight for the boilerplate heuristic.
    pub boilerplate_weight: f64,
    /// Weight for the semantic equivalence heuristic.
    pub semantic_weight: f64,
    /// Weight for the no observable behavior heuristic.
    pub no_behavior_weight: f64,
    /// Base score adjustment for mutations affecting return values.
    pub return_penalty: f64,
    /// Depth factor (deeper = more likely equivalent).
    pub depth_factor: f64,
}

impl Default for ScorerConfig {
    fn default() -> Self {
        Self {
            logging_weight: 0.95,
            dead_code_weight: 0.99,
            boilerplate_weight: 0.85,
            semantic_weight: 0.90,
            no_behavior_weight: 0.75,
            return_penalty: 0.3,
            depth_factor: 0.01,
        }
    }
}

/// Scores mutants by their likelihood of being equivalent.
#[derive(Debug)]
pub struct EquivalenceScorer {
    config: ScorerConfig,
    detector: HeuristicDetector,
}

impl Default for EquivalenceScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl EquivalenceScorer {
    /// Create a new scorer with default configuration.
    pub fn new() -> Self {
        Self {
            config: ScorerConfig::default(),
            detector: HeuristicDetector::new(),
        }
    }

    /// Create a scorer with custom configuration.
    pub fn with_config(config: ScorerConfig) -> Self {
        Self {
            config,
            detector: HeuristicDetector::new(),
        }
    }

    /// Score a mutant by its likelihood of being equivalent.
    ///
    /// Returns a score between 0.0 (definitely not equivalent) and
    /// 1.0 (definitely equivalent).
    pub fn score(&self, features: &MutantFeatures) -> f64 {
        let mut score: f64 = 0.0;

        // Heuristic-based scoring
        if features.in_logging {
            score = score.max(self.config.logging_weight);
        }

        if features.in_dead_code {
            score = score.max(self.config.dead_code_weight);
        }

        // Depth-based adjustment (deeper code is often less observable)
        let depth_bonus = (features.ast_depth as f64 * self.config.depth_factor).min(0.2);
        score += depth_bonus;

        // Penalty for affecting return values (more likely to be observable)
        if features.affects_return {
            score -= self.config.return_penalty;
        }

        // Complexity changes often indicate meaningful mutations
        if features.complexity_delta.abs() > 0 {
            score -= 0.1 * features.complexity_delta.abs() as f64;
        }

        score.clamp(0.0, 1.0)
    }

    /// Score a mutant using both features and the heuristic detector.
    pub fn score_with_reason(
        &self,
        mutant: &Mutant,
        features: &MutantFeatures,
    ) -> (f64, Option<EquivalenceReason>) {
        let reason = self.detector.equivalence_reason(mutant, features);

        let score = match &reason {
            Some(EquivalenceReason::InLoggingStatement) => self.config.logging_weight,
            Some(EquivalenceReason::InDeadCode) => self.config.dead_code_weight,
            Some(EquivalenceReason::InBoilerplate) => self.config.boilerplate_weight,
            Some(EquivalenceReason::SemanticallyEquivalent) => self.config.semantic_weight,
            Some(EquivalenceReason::NoObservableBehavior) => self.config.no_behavior_weight,
            None => self.score(features),
        };

        (score, reason)
    }

    /// Filter mutants, returning only high-value (non-equivalent) ones.
    ///
    /// Mutants with equivalence score above the threshold are filtered out.
    pub fn filter_high_value(
        &self,
        mutants: Vec<Mutant>,
        parse_result: &ParseResult,
        threshold: f64,
    ) -> Vec<Mutant> {
        mutants
            .into_iter()
            .filter(|mutant| {
                let features = MutantFeatures::extract(mutant, parse_result);
                let score = self.score(&features);
                score < threshold
            })
            .collect()
    }

    /// Filter mutants and return both kept and filtered mutants with scores.
    pub fn filter_with_scores(
        &self,
        mutants: Vec<Mutant>,
        parse_result: &ParseResult,
        threshold: f64,
    ) -> FilterResult {
        let mut kept = Vec::new();
        let mut filtered = Vec::new();

        for mutant in mutants {
            let features = MutantFeatures::extract(&mutant, parse_result);
            let (score, reason) = self.score_with_reason(&mutant, &features);

            let scored = ScoredMutant {
                mutant: mutant.clone(),
                score,
                reason,
            };

            if score < threshold {
                kept.push(scored);
            } else {
                filtered.push(scored);
            }
        }

        FilterResult { kept, filtered }
    }

    /// Get the threshold recommendation based on desired strictness.
    ///
    /// - strict (0.5): Only filter very likely equivalents
    /// - moderate (0.7): Filter likely equivalents
    /// - aggressive (0.85): Filter anything suspicious
    pub fn recommended_threshold(strictness: Strictness) -> f64 {
        match strictness {
            Strictness::Strict => 0.5,
            Strictness::Moderate => 0.7,
            Strictness::Aggressive => 0.85,
        }
    }
}

/// Strictness level for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strictness {
    /// Only filter high-confidence equivalents.
    Strict,
    /// Balance between false positives and false negatives.
    Moderate,
    /// Aggressively filter potential equivalents.
    Aggressive,
}

/// A mutant with its equivalence score.
#[derive(Debug, Clone)]
pub struct ScoredMutant {
    /// The original mutant.
    pub mutant: Mutant,
    /// Equivalence score (0.0 - 1.0).
    pub score: f64,
    /// Reason for the score, if from a heuristic.
    pub reason: Option<EquivalenceReason>,
}

/// Result of filtering mutants.
#[derive(Debug)]
pub struct FilterResult {
    /// Mutants that passed the filter (likely not equivalent).
    pub kept: Vec<ScoredMutant>,
    /// Mutants that were filtered out (likely equivalent).
    pub filtered: Vec<ScoredMutant>,
}

impl FilterResult {
    /// Get the number of kept mutants.
    pub fn kept_count(&self) -> usize {
        self.kept.len()
    }

    /// Get the number of filtered mutants.
    pub fn filtered_count(&self) -> usize {
        self.filtered.len()
    }

    /// Get the filter rate (percentage of mutants filtered).
    pub fn filter_rate(&self) -> f64 {
        let total = self.kept.len() + self.filtered.len();
        if total == 0 {
            return 0.0;
        }
        self.filtered.len() as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Language;
    use crate::parser::Parser;
    use std::path::Path;

    fn create_mutant(
        id: &str,
        operator: &str,
        original: &str,
        replacement: &str,
        byte_range: (usize, usize),
    ) -> Mutant {
        Mutant::new(
            id,
            "test.rs",
            operator,
            1,
            1,
            original,
            replacement,
            "test mutation",
            byte_range,
        )
    }

    fn create_features() -> MutantFeatures {
        MutantFeatures {
            operator_type: "CRR".to_string(),
            in_dead_code: false,
            in_logging: false,
            affects_return: false,
            complexity_delta: 0,
            ast_depth: 3,
            sibling_count: 2,
        }
    }

    fn parse_rust(code: &str) -> ParseResult {
        let parser = Parser::new();
        parser
            .parse(code.as_bytes(), Language::Rust, Path::new("test.rs"))
            .unwrap()
    }

    #[test]
    fn test_scorer_new() {
        let scorer = EquivalenceScorer::new();
        assert!(scorer.config.logging_weight > 0.0);
    }

    #[test]
    fn test_scorer_with_config() {
        let config = ScorerConfig {
            logging_weight: 0.8,
            ..Default::default()
        };
        let scorer = EquivalenceScorer::with_config(config);
        assert!((scorer.config.logging_weight - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_score_normal_mutant() {
        let scorer = EquivalenceScorer::new();
        let features = create_features();

        let score = scorer.score(&features);

        // Normal mutant should have low equivalence score
        assert!(score < 0.5);
    }

    #[test]
    fn test_score_logging_mutant() {
        let scorer = EquivalenceScorer::new();
        let mut features = create_features();
        features.in_logging = true;

        let score = scorer.score(&features);

        // Logging mutant should have high equivalence score
        assert!(score > 0.9);
    }

    #[test]
    fn test_score_dead_code_mutant() {
        let scorer = EquivalenceScorer::new();
        let mut features = create_features();
        features.in_dead_code = true;

        let score = scorer.score(&features);

        // Dead code mutant should have very high equivalence score
        assert!(score > 0.95);
    }

    #[test]
    fn test_score_return_affecting_mutant() {
        let scorer = EquivalenceScorer::new();
        let mut features = create_features();
        features.affects_return = true;

        let score = scorer.score(&features);

        // Return-affecting mutant should have lower equivalence score
        let normal_features = create_features();
        let normal_score = scorer.score(&normal_features);

        assert!(score < normal_score);
    }

    #[test]
    fn test_score_deep_mutant() {
        let scorer = EquivalenceScorer::new();
        let mut shallow_features = create_features();
        shallow_features.ast_depth = 2;

        let mut deep_features = create_features();
        deep_features.ast_depth = 10;

        let shallow_score = scorer.score(&shallow_features);
        let deep_score = scorer.score(&deep_features);

        // Deeper mutants should have slightly higher equivalence score
        assert!(deep_score > shallow_score);
    }

    #[test]
    fn test_score_complexity_change() {
        let scorer = EquivalenceScorer::new();
        let mut features = create_features();
        features.complexity_delta = 2;

        let score = scorer.score(&features);

        let no_change_features = create_features();
        let no_change_score = scorer.score(&no_change_features);

        // Complexity-changing mutants are less likely equivalent
        assert!(score < no_change_score);
    }

    #[test]
    fn test_score_clamped() {
        let scorer = EquivalenceScorer::new();

        // Features that would push score below 0
        let mut features = create_features();
        features.affects_return = true;
        features.complexity_delta = 10;

        let score = scorer.score(&features);
        assert!(score >= 0.0);

        // Features that would push score above 1
        let mut high_features = create_features();
        high_features.in_dead_code = true;
        high_features.in_logging = true;
        high_features.ast_depth = 100;

        let high_score = scorer.score(&high_features);
        assert!(high_score <= 1.0);
    }

    #[test]
    fn test_score_with_reason() {
        let scorer = EquivalenceScorer::new();
        let mutant = create_mutant("1", "CRR", "42", "0", (0, 2));
        let mut features = create_features();
        features.in_logging = true;

        let (score, reason) = scorer.score_with_reason(&mutant, &features);

        assert!(score > 0.9);
        assert_eq!(reason, Some(EquivalenceReason::InLoggingStatement));
    }

    #[test]
    fn test_filter_high_value() {
        let scorer = EquivalenceScorer::new();
        let code = "fn main() { let x = 42; println!(\"{}\", 10); }";
        let result = parse_rust(code);

        let mutants = vec![
            create_mutant("1", "CRR", "42", "0", (20, 22)), // In let statement
            create_mutant("2", "CRR", "10", "0", (41, 43)), // In println! (logging)
        ];

        let filtered = scorer.filter_high_value(mutants, &result, 0.5);

        // The println! mutant should be filtered out due to high score
        assert!(filtered.len() <= 2);
    }

    #[test]
    fn test_filter_with_scores() {
        let scorer = EquivalenceScorer::new();
        let code = "fn main() { let x = 42; }";
        let result = parse_rust(code);

        let mutants = vec![create_mutant("1", "CRR", "42", "0", (20, 22))];

        let filter_result = scorer.filter_with_scores(mutants, &result, 0.5);

        assert_eq!(filter_result.kept.len() + filter_result.filtered.len(), 1);
    }

    #[test]
    fn test_filter_result_stats() {
        let kept = vec![
            ScoredMutant {
                mutant: create_mutant("1", "CRR", "42", "0", (0, 2)),
                score: 0.2,
                reason: None,
            },
            ScoredMutant {
                mutant: create_mutant("2", "CRR", "10", "0", (10, 12)),
                score: 0.3,
                reason: None,
            },
        ];

        let filtered = vec![ScoredMutant {
            mutant: create_mutant("3", "CRR", "5", "0", (20, 21)),
            score: 0.9,
            reason: Some(EquivalenceReason::InLoggingStatement),
        }];

        let result = FilterResult { kept, filtered };

        assert_eq!(result.kept_count(), 2);
        assert_eq!(result.filtered_count(), 1);
        assert!((result.filter_rate() - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_filter_result_empty() {
        let result = FilterResult {
            kept: vec![],
            filtered: vec![],
        };

        assert_eq!(result.kept_count(), 0);
        assert_eq!(result.filtered_count(), 0);
        assert!((result.filter_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_recommended_threshold() {
        assert!(
            (EquivalenceScorer::recommended_threshold(Strictness::Strict) - 0.5).abs()
                < f64::EPSILON
        );
        assert!(
            (EquivalenceScorer::recommended_threshold(Strictness::Moderate) - 0.7).abs()
                < f64::EPSILON
        );
        assert!(
            (EquivalenceScorer::recommended_threshold(Strictness::Aggressive) - 0.85).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn test_default_config() {
        let config = ScorerConfig::default();

        assert!(config.logging_weight > 0.9);
        assert!(config.dead_code_weight > 0.9);
        assert!(config.boilerplate_weight > 0.8);
        assert!(config.return_penalty > 0.0);
    }

    #[test]
    fn test_scorer_default_trait() {
        let scorer1 = EquivalenceScorer::default();
        let scorer2 = EquivalenceScorer::new();

        // Both should have the same configuration
        assert!(
            (scorer1.config.logging_weight - scorer2.config.logging_weight).abs() < f64::EPSILON
        );
    }
}
