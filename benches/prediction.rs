use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ptyx::predict::EchoPredictor;

fn bench_prediction_ascii(c: &mut Criterion) {
    let inputs: &[&[u8]] = &[b"ls", b"pwd", b"echo hello world", b"cat /etc/hosts"];

    let mut group = c.benchmark_group("EchoPredictor::predict");
    for input in inputs {
        group.bench_with_input(
            BenchmarkId::from_parameter(input.len()),
            input,
            |b, input| {
                let mut predictor = EchoPredictor::new(3);
                b.iter(|| predictor.predict(black_box(input)));
            },
        );
    }
    group.finish();
}

fn bench_reconcile_hit(c: &mut Criterion) {
    c.bench_function("EchoPredictor::reconcile hit", |b| {
        b.iter(|| {
            let mut p = EchoPredictor::new(3);
            p.predict(b"hello");
            p.reconcile(black_box(b"hello"))
        })
    });
}

fn bench_reconcile_miss(c: &mut Criterion) {
    c.bench_function("EchoPredictor::reconcile miss", |b| {
        b.iter(|| {
            let mut p = EchoPredictor::new(10);
            p.predict(b"hello");
            p.reconcile(black_box(b"xyzzy"))
        })
    });
}

fn bench_check_output_for_raw_mode(c: &mut Criterion) {
    // Simulate scanning a 1KB output chunk for the alt-screen escape sequence.
    let chunk: Vec<u8> = (0u8..=255u8).cycle().take(1024).collect();
    c.bench_function("EchoPredictor::check_output_for_raw_mode 1kb", |b| {
        let mut p = EchoPredictor::new(3);
        b.iter(|| p.check_output_for_raw_mode(black_box(&chunk)));
    });
}

criterion_group!(
    benches,
    bench_prediction_ascii,
    bench_reconcile_hit,
    bench_reconcile_miss,
    bench_check_output_for_raw_mode,
);
criterion_main!(benches);
