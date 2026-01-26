//! Comprehensive benchmarks for all omen analyzers.
//!
//! Run with: cargo bench
//! Run specific benchmark: cargo bench -- complexity
//! Generate flamegraph: cargo bench --bench analyzers -- --profile-time=5

use std::process::Command;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tempfile::TempDir;

use omen::analyzers::{
    changes, churn, cohesion, complexity, deadcode, defect, duplicates, flags, graph, hotspot,
    ownership, repomap, satd, smells, tdg, temporal,
};
use omen::config::Config;
use omen::core::{AnalysisContext, Analyzer, FileSet};

/// Create a temporary git repository with sample files for benchmarking.
fn create_benchmark_repo(file_count: usize) -> TempDir {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let path = temp.path();

    // Initialize git repo
    Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .expect("Failed to init git");

    Command::new("git")
        .args(["config", "user.email", "bench@test.com"])
        .current_dir(path)
        .output()
        .expect("Failed to configure git");

    Command::new("git")
        .args(["config", "user.name", "Benchmark"])
        .current_dir(path)
        .output()
        .expect("Failed to configure git");

    // Create source files with varying complexity
    let src_dir = path.join("src");
    std::fs::create_dir_all(&src_dir).expect("Failed to create src dir");

    for i in 0..file_count {
        let content = generate_rust_file(i);
        let filename = format!("module_{}.rs", i);
        std::fs::write(src_dir.join(&filename), &content).expect("Failed to write file");

        // Add and commit each file to build git history
        Command::new("git")
            .args(["add", &format!("src/{}", filename)])
            .current_dir(path)
            .output()
            .expect("Failed to add file");

        Command::new("git")
            .args(["commit", "-m", &format!("Add module {}", i)])
            .current_dir(path)
            .output()
            .expect("Failed to commit");
    }

    temp
}

/// Generate a Rust file with varying complexity for benchmarking.
fn generate_rust_file(seed: usize) -> String {
    let complexity_level = seed % 5;
    let mut code = String::new();

    code.push_str(&format!("//! Module {} for benchmarking.\n\n", seed));

    // Generate functions with varying complexity
    for f in 0..(5 + complexity_level) {
        code.push_str(&generate_function(seed, f, complexity_level));
        code.push('\n');
    }

    code
}

/// Generate a function with specified complexity level.
fn generate_function(seed: usize, func_num: usize, complexity: usize) -> String {
    let mut func = format!(
        "/// Function {} in module {}.\npub fn function_{}_{} (x: i32, y: i32) -> i32 {{\n",
        func_num, seed, seed, func_num
    );

    // Add nested control flow based on complexity
    for depth in 0..complexity {
        func.push_str(&"    ".repeat(depth + 1));
        func.push_str(&format!("if x > {} {{\n", depth));
    }

    // Inner computation
    func.push_str(&"    ".repeat(complexity + 1));
    func.push_str("let result = x + y;\n");

    // TODO comment for SATD detection
    if seed.is_multiple_of(3) {
        func.push_str(&"    ".repeat(complexity + 1));
        func.push_str("// TODO: Optimize this calculation\n");
    }

    // FIXME comment for defect detection
    if seed.is_multiple_of(4) {
        func.push_str(&"    ".repeat(complexity + 1));
        func.push_str("// FIXME: Handle edge case\n");
    }

    // Close nested blocks
    for depth in (0..complexity).rev() {
        func.push_str(&"    ".repeat(depth + 1));
        func.push_str("}\n");
    }

    func.push_str("    result\n}\n");
    func
}

/// Benchmark file set creation (file discovery).
fn bench_file_discovery(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_discovery");

    for size in [10, 50, 100].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            b.iter(|| {
                let files = FileSet::from_path(temp.path(), &config).unwrap();
                black_box(files.len())
            });
        });
    }

    group.finish();
}

/// Benchmark the complexity analyzer.
fn bench_complexity(c: &mut Criterion) {
    let mut group = c.benchmark_group("complexity");

    for size in [10, 50, 100].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = complexity::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_functions)
            });
        });
    }

    group.finish();
}

/// Benchmark the SATD analyzer.
fn bench_satd(c: &mut Criterion) {
    let mut group = c.benchmark_group("satd");

    for size in [10, 50, 100].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = satd::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_items)
            });
        });
    }

    group.finish();
}

/// Benchmark the deadcode analyzer.
fn bench_deadcode(c: &mut Criterion) {
    let mut group = c.benchmark_group("deadcode");

    for size in [10, 50].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = deadcode::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_items)
            });
        });
    }

    group.finish();
}

/// Benchmark the duplicates (clones) analyzer.
fn bench_duplicates(c: &mut Criterion) {
    let mut group = c.benchmark_group("duplicates");
    group.sample_size(20); // Fewer samples for expensive operation

    for size in [10, 30].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = duplicates::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_clones)
            });
        });
    }

    group.finish();
}

/// Benchmark the graph analyzer.
fn bench_graph(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph");

    for size in [10, 50].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = graph::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_nodes)
            });
        });
    }

    group.finish();
}

/// Benchmark the cohesion analyzer.
fn bench_cohesion(c: &mut Criterion) {
    let mut group = c.benchmark_group("cohesion");

    for size in [10, 50].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = cohesion::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_classes)
            });
        });
    }

    group.finish();
}

/// Benchmark the smells analyzer.
fn bench_smells(c: &mut Criterion) {
    let mut group = c.benchmark_group("smells");

    for size in [10, 50].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = smells::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_smells)
            });
        });
    }

    group.finish();
}

/// Benchmark the repomap analyzer.
fn bench_repomap(c: &mut Criterion) {
    let mut group = c.benchmark_group("repomap");

    for size in [10, 49].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = repomap::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_symbols)
            });
        });
    }

    group.finish();
}

/// Benchmark the flags analyzer.
fn bench_flags(c: &mut Criterion) {
    let mut group = c.benchmark_group("flags");

    for size in [10, 50].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = flags::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_flags)
            });
        });
    }

    group.finish();
}

/// Benchmark the TDG analyzer.
fn bench_tdg(c: &mut Criterion) {
    let mut group = c.benchmark_group("tdg");

    for size in [10, 50].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx = AnalysisContext::new(&files, &config, Some(temp.path()));

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = tdg::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.total_files)
            });
        });
    }

    group.finish();
}

// ============================================================================
// Git-dependent benchmarks (churn, changes, defect, ownership, temporal, hotspot)
// These are separated because they require git history
// ============================================================================

/// Benchmark the churn analyzer (git-dependent).
fn bench_churn(c: &mut Criterion) {
    let mut group = c.benchmark_group("churn");
    group.sample_size(20); // Git operations are slower

    for size in [10, 30].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx =
            AnalysisContext::new(&files, &config, Some(temp.path())).with_git_path(temp.path());

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = churn::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_files_changed)
            });
        });
    }

    group.finish();
}

/// Benchmark the changes/JIT analyzer (git-dependent).
fn bench_changes(c: &mut Criterion) {
    let mut group = c.benchmark_group("changes");
    group.sample_size(20);

    for size in [10, 30].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx =
            AnalysisContext::new(&files, &config, Some(temp.path())).with_git_path(temp.path());

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("commits", size), size, |b, _| {
            let analyzer = changes::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_commits)
            });
        });
    }

    group.finish();
}

/// Benchmark the defect analyzer (git-dependent).
fn bench_defect(c: &mut Criterion) {
    let mut group = c.benchmark_group("defect");
    group.sample_size(10); // Very slow due to multiple git calls per file

    for size in [5, 10].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx =
            AnalysisContext::new(&files, &config, Some(temp.path())).with_git_path(temp.path());

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = defect::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_files)
            });
        });
    }

    group.finish();
}

/// Benchmark the ownership analyzer (git-dependent).
fn bench_ownership(c: &mut Criterion) {
    let mut group = c.benchmark_group("ownership");
    group.sample_size(20);

    for size in [10, 30].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx =
            AnalysisContext::new(&files, &config, Some(temp.path())).with_git_path(temp.path());

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = ownership::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_files)
            });
        });
    }

    group.finish();
}

/// Benchmark the temporal coupling analyzer (git-dependent).
fn bench_temporal(c: &mut Criterion) {
    let mut group = c.benchmark_group("temporal");
    group.sample_size(20);

    for size in [10, 30].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx =
            AnalysisContext::new(&files, &config, Some(temp.path())).with_git_path(temp.path());

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = temporal::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_couplings)
            });
        });
    }

    group.finish();
}

/// Benchmark the hotspot analyzer (git-dependent, combines churn and complexity).
fn bench_hotspot(c: &mut Criterion) {
    let mut group = c.benchmark_group("hotspot");
    group.sample_size(20);

    for size in [10, 30].iter() {
        let temp = create_benchmark_repo(*size);
        let config = Config::default();
        let files = FileSet::from_path(temp.path(), &config).unwrap();
        let ctx =
            AnalysisContext::new(&files, &config, Some(temp.path())).with_git_path(temp.path());

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("files", size), size, |b, _| {
            let analyzer = hotspot::Analyzer::new();
            b.iter(|| {
                let result = analyzer.analyze(&ctx).unwrap();
                black_box(result.summary.total_hotspots)
            });
        });
    }

    group.finish();
}

// Group benchmarks: non-git analyzers first (faster), then git-dependent
criterion_group!(
    name = fast_benches;
    config = Criterion::default().sample_size(50);
    targets =
        bench_file_discovery,
        bench_complexity,
        bench_satd,
        bench_deadcode,
        bench_graph,
        bench_cohesion,
        bench_smells,
        bench_repomap,
        bench_flags,
        bench_tdg
);

criterion_group!(
    name = git_benches;
    config = Criterion::default().sample_size(20);
    targets =
        bench_churn,
        bench_changes,
        bench_defect,
        bench_ownership,
        bench_temporal,
        bench_hotspot
);

criterion_group!(
    name = slow_benches;
    config = Criterion::default().sample_size(10);
    targets = bench_duplicates
);

criterion_main!(fast_benches, git_benches, slow_benches);
