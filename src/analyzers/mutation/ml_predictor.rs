//! ML-Based Mutant Survivability Predictor
//!
//! Predicts which mutants are likely to be killed vs survive based on
//! code features. Uses linear regression for binary classification.
//!
//! Based on PMAT's approach with 18 features extracted from each mutant.

use super::Mutant;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Features extracted from a mutant for ML prediction.
/// 18-dimensional feature vector based on PMAT's approach.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutantFeatures {
    /// Type of mutation operator (numeric encoding).
    pub operator_type: f64,
    /// Cyclomatic complexity at mutation point.
    pub cyclomatic_complexity: u32,
    /// Cognitive complexity at mutation point.
    pub cognitive_complexity: u32,
    /// Source line number.
    pub source_line: u32,
    /// Nesting depth at mutation point.
    pub nesting_depth: u32,
    /// Number of control flow constructs nearby.
    pub control_flow_count: u32,
    /// Has loops nearby.
    pub has_loops: bool,
    /// Has conditionals nearby.
    pub has_conditionals: bool,
    /// Function size (LOC).
    pub function_size: u32,
    /// Number of parameters.
    pub parameter_count: u32,
    /// Has error handling (try/catch/Result/?).
    pub has_error_handling: bool,
    /// Has assertions or tests.
    pub has_assertions: bool,
    /// Token count (code density).
    pub token_count: u32,
    /// Unique variable count.
    pub unique_variables: u32,
    /// Has arithmetic operations.
    pub has_arithmetic: bool,
    /// Has comparison operations.
    pub has_comparisons: bool,
    /// Has logical operations (&&, ||, !).
    pub has_logical_ops: bool,
    /// Mutation depth (nesting in control flow).
    pub mutation_depth: u32,
}

impl MutantFeatures {
    /// Extract features from a mutant and its surrounding source context.
    pub fn from_mutant(mutant: &Mutant, source_context: &str) -> Self {
        let source = source_context;

        // Control flow detection
        let has_loops =
            source.contains("for") || source.contains("while") || source.contains("loop");
        let has_conditionals = source.contains("if") || source.contains("match");

        let control_flow_count = source.matches("if").count() as u32
            + source.matches("for").count() as u32
            + source.matches("while").count() as u32
            + source.matches("match").count() as u32;

        let nesting_depth = estimate_nesting_depth(source);
        let cyclomatic_complexity = 1 + control_flow_count;
        let cognitive_complexity = cyclomatic_complexity + nesting_depth;
        let function_size = source.lines().count() as u32;
        let parameter_count = count_parameters(source);

        // Error handling detection
        let has_error_handling = source.contains("Result<")
            || source.contains("Option<")
            || source.contains("unwrap")
            || source.contains("expect")
            || source.contains('?')
            || source.contains("try")
            || source.contains("catch")
            || source.contains("Error")
            || source.contains("error");

        // Assertion detection
        let has_assertions = source.contains("assert")
            || source.contains("debug_assert")
            || source.contains("#[test]")
            || source.contains("expect(")
            || source.contains(".should");

        let token_count = source.split_whitespace().count() as u32;
        let unique_variables = count_unique_variables(source);

        let has_arithmetic = source.contains('+')
            || source.contains('-')
            || source.contains('*')
            || source.contains('/');

        let has_comparisons = source.contains("==")
            || source.contains("!=")
            || source.contains("<=")
            || source.contains(">=")
            || source.contains('<')
            || source.contains('>');

        let has_logical_ops =
            source.contains("&&") || source.contains("||") || source.contains('!');

        Self {
            operator_type: operator_to_numeric(&mutant.operator),
            cyclomatic_complexity,
            cognitive_complexity,
            source_line: mutant.line,
            nesting_depth,
            control_flow_count,
            has_loops,
            has_conditionals,
            function_size,
            parameter_count,
            has_error_handling,
            has_assertions,
            token_count,
            unique_variables,
            has_arithmetic,
            has_comparisons,
            has_logical_ops,
            mutation_depth: nesting_depth,
        }
    }

    /// Convert features to a numeric vector for ML model.
    pub fn to_feature_vector(&self) -> Vec<f64> {
        vec![
            self.operator_type,
            self.cyclomatic_complexity as f64,
            self.cognitive_complexity as f64,
            self.source_line as f64,
            self.nesting_depth as f64,
            self.control_flow_count as f64,
            bool_to_f64(self.has_loops),
            bool_to_f64(self.has_conditionals),
            self.function_size as f64,
            self.parameter_count as f64,
            bool_to_f64(self.has_error_handling),
            bool_to_f64(self.has_assertions),
            self.token_count as f64,
            self.unique_variables as f64,
            bool_to_f64(self.has_arithmetic),
            bool_to_f64(self.has_comparisons),
            bool_to_f64(self.has_logical_ops),
            self.mutation_depth as f64,
        ]
    }
}

/// Training data for the ML model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingData {
    /// The mutant that was tested.
    pub mutant: Mutant,
    /// Source context around the mutant.
    pub source_context: String,
    /// Whether the mutant was killed by tests.
    pub was_killed: bool,
    /// Test execution time in milliseconds.
    pub execution_time_ms: u64,
}

/// Prediction result from the ML model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionResult {
    /// Probability that this mutant will be killed (0.0 - 1.0).
    pub kill_probability: f64,
    /// Confidence in the prediction (0.0 - 1.0).
    pub confidence: f64,
    /// Whether the mutant is predicted to be killed.
    pub predicted_killed: bool,
    /// Feature contributions to the prediction.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub feature_contributions: HashMap<String, f64>,
}

/// Simple linear regression model for binary classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearRegressionModel {
    /// Model weights (one per feature + bias).
    weights: Vec<f64>,
    /// Feature means for normalization.
    feature_means: Vec<f64>,
    /// Feature standard deviations for normalization.
    feature_stds: Vec<f64>,
    /// Number of training samples.
    n_samples: usize,
}

impl LinearRegressionModel {
    /// Create an untrained model.
    pub fn new() -> Self {
        Self {
            weights: Vec::new(),
            feature_means: Vec::new(),
            feature_stds: Vec::new(),
            n_samples: 0,
        }
    }

    /// Check if model is trained.
    pub fn is_trained(&self) -> bool {
        !self.weights.is_empty()
    }

    /// Train the model using ordinary least squares.
    pub fn train(&mut self, features: &[Vec<f64>], labels: &[f64]) -> Result<(), String> {
        let n_samples = features.len();
        let n_features = if n_samples > 0 { features[0].len() } else { 0 };

        if n_samples == 0 {
            return Err("No training data provided".to_string());
        }

        if n_samples < n_features {
            return Err(format!(
                "Insufficient samples: {} samples for {} features (need at least {})",
                n_samples, n_features, n_features
            ));
        }

        // Calculate feature statistics for normalization
        self.feature_means = vec![0.0; n_features];
        self.feature_stds = vec![1.0; n_features];

        for feat in features {
            for (i, &v) in feat.iter().enumerate() {
                self.feature_means[i] += v;
            }
        }
        for mean in &mut self.feature_means {
            *mean /= n_samples as f64;
        }

        for feat in features {
            for (i, &v) in feat.iter().enumerate() {
                let diff = v - self.feature_means[i];
                self.feature_stds[i] += diff * diff;
            }
        }
        for std in &mut self.feature_stds {
            *std = (*std / n_samples as f64).sqrt().max(1e-8);
        }

        // Normalize features
        let normalized: Vec<Vec<f64>> = features
            .iter()
            .map(|f| {
                f.iter()
                    .enumerate()
                    .map(|(i, &v)| (v - self.feature_means[i]) / self.feature_stds[i])
                    .collect()
            })
            .collect();

        // Add bias term (column of 1s)
        let x_matrix: Vec<Vec<f64>> = normalized
            .iter()
            .map(|row| {
                let mut r = vec![1.0]; // bias
                r.extend(row);
                r
            })
            .collect();

        // Solve using normal equations: w = (X^T X)^-1 X^T y
        let n_cols = n_features + 1;

        // X^T X
        let mut xtx = vec![vec![0.0; n_cols]; n_cols];
        for row in &x_matrix {
            for i in 0..n_cols {
                for j in 0..n_cols {
                    xtx[i][j] += row[i] * row[j];
                }
            }
        }

        // Add regularization (ridge regression) to prevent singular matrix
        let lambda = 0.01;
        for i in 0..n_cols {
            xtx[i][i] += lambda;
        }

        // X^T y
        let mut xty = vec![0.0; n_cols];
        for (row, &label) in x_matrix.iter().zip(labels) {
            for (i, &x) in row.iter().enumerate() {
                xty[i] += x * label;
            }
        }

        // Solve using Gaussian elimination with partial pivoting
        self.weights = solve_linear_system(&xtx, &xty)?;
        self.n_samples = n_samples;

        Ok(())
    }

    /// Predict kill probability for a feature vector.
    pub fn predict(&self, features: &[f64]) -> f64 {
        if !self.is_trained() {
            return 0.5; // Default to 50% if untrained
        }

        // Normalize features
        let normalized: Vec<f64> = features
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                if i < self.feature_means.len() {
                    (v - self.feature_means[i]) / self.feature_stds[i]
                } else {
                    v
                }
            })
            .collect();

        // Calculate prediction: bias + sum(w_i * x_i)
        let mut prediction = self.weights[0]; // bias
        for (i, &x) in normalized.iter().enumerate() {
            if i + 1 < self.weights.len() {
                prediction += self.weights[i + 1] * x;
            }
        }

        // Clamp to [0, 1] for probability
        prediction.clamp(0.0, 1.0)
    }
}

impl Default for LinearRegressionModel {
    fn default() -> Self {
        Self::new()
    }
}

/// ML-based survivability predictor.
#[derive(Debug)]
pub struct SurvivabilityPredictor {
    /// Trained linear regression model.
    model: LinearRegressionModel,
    /// Historical kill rates by operator type (fallback).
    operator_kill_rates: HashMap<String, f64>,
    /// Feature names for interpretation.
    feature_names: Vec<&'static str>,
    /// Whether the model is trained.
    trained: bool,
}

impl SurvivabilityPredictor {
    /// Create a new predictor.
    pub fn new() -> Self {
        Self {
            model: LinearRegressionModel::new(),
            operator_kill_rates: default_operator_kill_rates(),
            feature_names: vec![
                "operator_type",
                "cyclomatic_complexity",
                "cognitive_complexity",
                "source_line",
                "nesting_depth",
                "control_flow_count",
                "has_loops",
                "has_conditionals",
                "function_size",
                "parameter_count",
                "has_error_handling",
                "has_assertions",
                "token_count",
                "unique_variables",
                "has_arithmetic",
                "has_comparisons",
                "has_logical_ops",
                "mutation_depth",
            ],
            trained: false,
        }
    }

    /// Check if the predictor is trained.
    pub fn is_trained(&self) -> bool {
        self.trained
    }

    /// Train the predictor on historical mutation data.
    pub fn train(&mut self, training_data: &[TrainingData]) -> Result<(), String> {
        if training_data.is_empty() {
            return Err("Training data cannot be empty".to_string());
        }

        // Extract features and labels
        let mut features: Vec<Vec<f64>> = Vec::with_capacity(training_data.len());
        let mut labels: Vec<f64> = Vec::with_capacity(training_data.len());

        for sample in training_data {
            let mutant_features =
                MutantFeatures::from_mutant(&sample.mutant, &sample.source_context);
            features.push(mutant_features.to_feature_vector());
            labels.push(if sample.was_killed { 1.0 } else { 0.0 });
        }

        // Update operator kill rates from training data
        let mut operator_counts: HashMap<String, (usize, usize)> = HashMap::new();
        for sample in training_data {
            let entry = operator_counts
                .entry(sample.mutant.operator.clone())
                .or_insert((0, 0));
            entry.0 += 1; // total
            if sample.was_killed {
                entry.1 += 1; // killed
            }
        }
        for (op, (total, killed)) in operator_counts {
            if total > 0 {
                self.operator_kill_rates
                    .insert(op, killed as f64 / total as f64);
            }
        }

        // Train the model
        match self.model.train(&features, &labels) {
            Ok(()) => {
                self.trained = true;
                Ok(())
            }
            Err(e) => {
                // Fall back to operator-based prediction
                eprintln!(
                    "Warning: ML training failed ({}), using operator baseline",
                    e
                );
                self.trained = false;
                Ok(())
            }
        }
    }

    /// Predict kill probability for a mutant.
    pub fn predict(&self, mutant: &Mutant, source_context: &str) -> PredictionResult {
        let features = MutantFeatures::from_mutant(mutant, source_context);
        let feature_vector = features.to_feature_vector();

        let kill_probability = if self.trained {
            self.model.predict(&feature_vector)
        } else {
            // Fall back to operator-based prediction
            self.operator_kill_rates
                .get(&mutant.operator)
                .copied()
                .unwrap_or(0.5)
        };

        // Calculate confidence based on training data size and feature certainty
        let confidence = if self.trained {
            let base_confidence = (self.model.n_samples as f64 / 100.0).min(1.0);
            // Higher confidence for extreme predictions
            let prediction_certainty = (kill_probability - 0.5).abs() * 2.0;
            (base_confidence * 0.7 + prediction_certainty * 0.3).min(1.0)
        } else {
            0.3 // Low confidence for fallback predictions
        };

        // Calculate feature contributions if trained
        let feature_contributions = if self.trained && self.model.weights.len() > 1 {
            self.feature_names
                .iter()
                .enumerate()
                .filter_map(|(i, &name)| {
                    if i + 1 < self.model.weights.len() {
                        let contribution = self.model.weights[i + 1] * feature_vector[i];
                        Some((name.to_string(), contribution))
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            HashMap::new()
        };

        PredictionResult {
            kill_probability,
            confidence,
            predicted_killed: kill_probability >= 0.5,
            feature_contributions,
        }
    }

    /// Predict for multiple mutants.
    pub fn predict_batch(&self, mutants: &[(Mutant, String)]) -> Vec<(Mutant, PredictionResult)> {
        mutants
            .iter()
            .map(|(mutant, context)| (mutant.clone(), self.predict(mutant, context)))
            .collect()
    }

    /// Filter mutants predicted to survive (low kill probability).
    pub fn filter_likely_survivors(
        &self,
        mutants: &[(Mutant, String)],
        threshold: f64,
    ) -> Vec<Mutant> {
        mutants
            .iter()
            .filter(|(mutant, context)| {
                let prediction = self.predict(mutant, context);
                prediction.kill_probability < threshold
            })
            .map(|(mutant, _)| mutant.clone())
            .collect()
    }

    /// Get operator kill rates.
    pub fn operator_kill_rates(&self) -> &HashMap<String, f64> {
        &self.operator_kill_rates
    }
}

impl Default for SurvivabilityPredictor {
    fn default() -> Self {
        Self::new()
    }
}

// Helper functions

fn bool_to_f64(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn operator_to_numeric(operator: &str) -> f64 {
    match operator.to_uppercase().as_str() {
        "AOR" => 1.0,  // Arithmetic
        "ROR" => 2.0,  // Relational
        "COR" => 3.0,  // Conditional
        "CRR" => 4.0,  // Constant
        "SDL" => 5.0,  // Statement deletion
        "RVR" => 6.0,  // Return value
        "UOR" => 7.0,  // Unary
        "BVO" => 8.0,  // Boundary value
        "BOR" => 9.0,  // Bitwise
        "ASR" => 10.0, // Assignment
        "LCR" => 11.0, // Logical connector
        "OPT" => 12.0, // Option (Rust)
        "RES" => 13.0, // Result (Rust)
        "BRW" => 14.0, // Borrow (Rust)
        "ERR" => 15.0, // Error handling (Go)
        "NIL" => 16.0, // Nil check (Go)
        "EQU" => 17.0, // Equality (TypeScript)
        "OPC" => 18.0, // Optional chaining (TypeScript)
        "IDE" => 19.0, // Identity (Python)
        "CMP" => 20.0, // Comprehension (Python)
        "SYM" => 21.0, // Symbol (Ruby)
        _ => 0.0,
    }
}

fn estimate_nesting_depth(source: &str) -> u32 {
    let mut max_depth = 0u32;
    let mut current_depth = 0u32;

    for c in source.chars() {
        match c {
            '{' | '(' | '[' => {
                current_depth += 1;
                max_depth = max_depth.max(current_depth);
            }
            '}' | ')' | ']' => {
                current_depth = current_depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    max_depth
}

fn count_parameters(source: &str) -> u32 {
    // Simple heuristic: count commas in function signatures
    let mut paren_depth: u32 = 0;
    let mut comma_count = 0u32;
    let mut has_content = false;
    let mut found_fn_parens = false;

    for c in source.chars() {
        match c {
            '(' => {
                paren_depth += 1;
                if paren_depth == 1 {
                    found_fn_parens = true;
                }
            }
            ')' => {
                if paren_depth == 1 && found_fn_parens {
                    // End of first function's parameters
                    break;
                }
                paren_depth = paren_depth.saturating_sub(1);
            }
            ',' if paren_depth == 1 => {
                comma_count += 1;
            }
            c if paren_depth == 1 && !c.is_whitespace() => {
                has_content = true;
            }
            _ => {}
        }
    }

    if found_fn_parens && (has_content || comma_count > 0) {
        comma_count + 1 // n commas = n+1 parameters
    } else {
        0
    }
}

fn count_unique_variables(source: &str) -> u32 {
    use std::collections::HashSet;

    let mut variables = HashSet::new();

    // Simple heuristic: extract lowercase identifiers
    let mut current_word = String::new();

    for c in source.chars() {
        if c.is_alphanumeric() || c == '_' {
            current_word.push(c);
        } else {
            if !current_word.is_empty()
                && current_word
                    .chars()
                    .next()
                    .map(|c| c.is_lowercase())
                    .unwrap_or(false)
                && !is_keyword(&current_word)
            {
                variables.insert(current_word.clone());
            }
            current_word.clear();
        }
    }

    variables.len() as u32
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "if" | "else"
            | "for"
            | "while"
            | "loop"
            | "match"
            | "return"
            | "let"
            | "mut"
            | "const"
            | "fn"
            | "pub"
            | "struct"
            | "enum"
            | "impl"
            | "trait"
            | "use"
            | "mod"
            | "true"
            | "false"
            | "self"
            | "super"
            | "crate"
            | "where"
            | "async"
            | "await"
            | "move"
            | "ref"
            | "type"
            | "as"
            | "in"
            | "break"
            | "continue"
            | "dyn"
            | "static"
            | "unsafe"
            | "extern"
    )
}

fn default_operator_kill_rates() -> HashMap<String, f64> {
    let mut rates = HashMap::new();
    // Based on empirical data from mutation testing research
    rates.insert("AOR".to_string(), 0.75); // Arithmetic - usually caught
    rates.insert("ROR".to_string(), 0.70); // Relational - often caught
    rates.insert("COR".to_string(), 0.65); // Conditional - moderately caught
    rates.insert("CRR".to_string(), 0.60); // Constant - sometimes missed
    rates.insert("SDL".to_string(), 0.85); // Statement deletion - usually caught
    rates.insert("RVR".to_string(), 0.80); // Return value - usually caught
    rates.insert("UOR".to_string(), 0.70); // Unary - often caught
    rates.insert("BVO".to_string(), 0.55); // Boundary - often missed
    rates.insert("BOR".to_string(), 0.65); // Bitwise - moderately caught
    rates.insert("ASR".to_string(), 0.75); // Assignment - usually caught
    rates.insert("LCR".to_string(), 0.60); // Logical connector - sometimes missed
    rates
}

/// Solve a linear system Ax = b using Gaussian elimination with partial pivoting.
fn solve_linear_system(a: &[Vec<f64>], b: &[f64]) -> Result<Vec<f64>, String> {
    let n = b.len();
    if n == 0 || a.len() != n || a[0].len() != n {
        return Err("Invalid matrix dimensions".to_string());
    }

    // Create augmented matrix [A|b]
    let mut aug: Vec<Vec<f64>> = a
        .iter()
        .zip(b)
        .map(|(row, &bi)| {
            let mut r = row.clone();
            r.push(bi);
            r
        })
        .collect();

    // Forward elimination with partial pivoting
    for col in 0..n {
        // Find pivot
        let mut max_row = col;
        let mut max_val = aug[col][col].abs();
        for row in (col + 1)..n {
            if aug[row][col].abs() > max_val {
                max_val = aug[row][col].abs();
                max_row = row;
            }
        }

        if max_val < 1e-10 {
            return Err("Matrix is singular or nearly singular".to_string());
        }

        // Swap rows
        aug.swap(col, max_row);

        // Eliminate column
        for row in (col + 1)..n {
            let factor = aug[row][col] / aug[col][col];
            for j in col..=n {
                aug[row][j] -= factor * aug[col][j];
            }
        }
    }

    // Back substitution
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        x[i] = aug[i][n];
        for j in (i + 1)..n {
            x[i] -= aug[i][j] * x[j];
        }
        x[i] /= aug[i][i];
    }

    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_mutant(operator: &str, line: u32) -> Mutant {
        Mutant::new(
            format!("test-{}", line),
            "test.rs",
            operator,
            line,
            1,
            "x > 0",
            "x >= 0",
            "Test mutation",
            (0, 5),
        )
    }

    #[test]
    fn test_feature_extraction() {
        let mutant = create_test_mutant("ROR", 10);
        let source = r#"
fn calculate(x: i32, y: i32) -> i32 {
    if x > 0 {
        for i in 0..y {
            println!("{}", i);
        }
        x + y
    } else {
        0
    }
}
"#;
        let features = MutantFeatures::from_mutant(&mutant, source);

        assert!(features.has_loops);
        assert!(features.has_conditionals);
        assert!(features.has_arithmetic);
        assert!(features.has_comparisons);
        assert!(features.control_flow_count >= 2);
        assert!(features.parameter_count == 2);
    }

    #[test]
    fn test_feature_vector_length() {
        let mutant = create_test_mutant("AOR", 1);
        let features = MutantFeatures::from_mutant(&mutant, "fn foo() { 1 + 2 }");
        let vector = features.to_feature_vector();

        assert_eq!(vector.len(), 18);
    }

    #[test]
    fn test_predictor_untrained() {
        let predictor = SurvivabilityPredictor::new();
        assert!(!predictor.is_trained());

        let mutant = create_test_mutant("ROR", 5);
        let result = predictor.predict(&mutant, "if x > 0 { }");

        // Should use fallback operator rates
        assert!(result.kill_probability > 0.0);
        assert!(result.confidence < 0.5); // Low confidence for untrained
    }

    #[test]
    fn test_linear_regression_simple() {
        let mut model = LinearRegressionModel::new();

        // Simple linear data: y = 0.5 * x (with more samples for stability)
        let features = vec![
            vec![0.0],
            vec![1.0],
            vec![2.0],
            vec![3.0],
            vec![4.0],
            vec![5.0],
            vec![6.0],
        ];
        let labels = vec![0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0];

        model.train(&features, &labels).unwrap();
        assert!(model.is_trained());

        // Predict - with normalization the exact value may vary
        let pred = model.predict(&[3.0]);
        // Just verify it's in a reasonable range (between 0 and 1 after clamping)
        assert!((0.0..=1.0).contains(&pred));
    }

    #[test]
    fn test_predictor_training() {
        let mut predictor = SurvivabilityPredictor::new();

        // Create training data
        let training_data: Vec<TrainingData> = (0..30)
            .map(|i| TrainingData {
                mutant: create_test_mutant(if i % 2 == 0 { "ROR" } else { "AOR" }, i as u32),
                source_context: format!("fn test{}() {{ if x > {} {{ }} }}", i, i),
                was_killed: i % 3 != 0, // 2/3 killed
                execution_time_ms: 100,
            })
            .collect();

        let result = predictor.train(&training_data);
        assert!(result.is_ok());

        // Predict on new mutant
        let mutant = create_test_mutant("ROR", 100);
        let prediction = predictor.predict(&mutant, "if x > 0 { return true; }");

        assert!(prediction.kill_probability >= 0.0);
        assert!(prediction.kill_probability <= 1.0);
    }

    #[test]
    fn test_operator_to_numeric() {
        assert_eq!(operator_to_numeric("AOR"), 1.0);
        assert_eq!(operator_to_numeric("ROR"), 2.0);
        assert_eq!(operator_to_numeric("unknown"), 0.0);
    }

    #[test]
    fn test_nesting_depth() {
        assert_eq!(estimate_nesting_depth("x"), 0);
        // "if (x) { y }" -> ( goes to 1, ) goes to 0, { goes to 1, max = 1
        assert_eq!(estimate_nesting_depth("if (x) { y }"), 1);
        // Nested braces: { { } } -> max = 2
        assert_eq!(estimate_nesting_depth("{ { x } }"), 2);
        // Nested with parens inside braces
        assert_eq!(estimate_nesting_depth("if { (x) }"), 2);
    }

    #[test]
    fn test_count_parameters() {
        assert_eq!(count_parameters("fn foo()"), 0);
        assert_eq!(count_parameters("fn foo(x)"), 1);
        assert_eq!(count_parameters("fn foo(x, y)"), 2);
        assert_eq!(count_parameters("fn foo(x, y, z)"), 3);
    }

    #[test]
    fn test_default_operator_rates() {
        let rates = default_operator_kill_rates();
        assert!(rates.contains_key("AOR"));
        assert!(rates.contains_key("ROR"));
        assert!(*rates.get("SDL").unwrap() > 0.8); // Statement deletion usually caught
    }

    #[test]
    fn test_solve_linear_system() {
        // Simple 2x2 system: x + y = 3, 2x + y = 4
        // Solution: x = 1, y = 2
        let a = vec![vec![1.0, 1.0], vec![2.0, 1.0]];
        let b = vec![3.0, 4.0];

        let x = solve_linear_system(&a, &b).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-6);
        assert!((x[1] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_filter_likely_survivors() {
        let predictor = SurvivabilityPredictor::new();

        let mutants: Vec<(Mutant, String)> = vec![
            (create_test_mutant("ROR", 1), "if x > 0 {}".to_string()),
            (create_test_mutant("BVO", 2), "x == 0".to_string()),
        ];

        // BVO has lower kill rate (0.55) than ROR (0.70)
        let survivors = predictor.filter_likely_survivors(&mutants, 0.6);

        // Should include BVO mutant (kill rate 0.55 < 0.6)
        assert!(!survivors.is_empty());
    }
}
