//! Benchmarks for analyzers.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn complexity_benchmark(c: &mut Criterion) {
    // TODO: Implement benchmarks
    c.bench_function("complexity_small", |b| b.iter(|| black_box(1 + 1)));
}

criterion_group!(benches, complexity_benchmark);
criterion_main!(benches);
