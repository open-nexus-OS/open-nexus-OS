//! CONTEXT: Latency benchmark suite
//! INTENT: Performance measurement of core operations
//! IDL (target): spin(count), measure(func), report()
//! DEPS: criterion (benchmarking), bench (spin function)
//! READINESS: Benchmark suite; no service dependencies
//! TESTS: Spin function latency; measurement accuracy
use bench::spin;
use criterion::{criterion_group, criterion_main, Criterion};

fn latency_bench(c: &mut Criterion) {
    c.bench_function("spin-100", |b| b.iter(|| spin(100)));
}

criterion_group!(benches, latency_bench);
criterion_main!(benches);
