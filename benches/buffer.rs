use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ptyx::buffer::InputBuffer;
use ptyx::metrics::SessionMetrics;
use std::time::Duration;

// --- Phase 1 baselines ---

fn bench_push_single_byte(c: &mut Criterion) {
    c.bench_function("push_single_byte", |b| {
        let mut buf = InputBuffer::new(Duration::from_millis(20), 512);
        b.iter(|| {
            buf.push(black_box(b'a'));
            if buf.len() >= 400 {
                buf.take();
            }
        });
    });
}

fn bench_push_1000_bytes(c: &mut Criterion) {
    c.bench_function("push_1000_bytes", |b| {
        b.iter(|| {
            let mut buf = InputBuffer::new(Duration::from_millis(20), 2048);
            for _ in 0..1000 {
                buf.push(black_box(b'x'));
            }
            black_box(buf.take());
        });
    });
}

fn bench_take(c: &mut Criterion) {
    c.bench_function("take", |b| {
        let mut buf = InputBuffer::new(Duration::from_millis(20), 2048);
        for _ in 0..100 {
            buf.push(b'x');
        }
        b.iter(|| {
            if buf.is_empty() {
                for _ in 0..100 {
                    buf.push(b'x');
                }
            }
            black_box(buf.take());
        });
    });
}

// --- Phase 2 additions ---

fn bench_passthrough_overhead(c: &mut Criterion) {
    c.bench_function("passthrough_overhead", |b| {
        let mut buf = InputBuffer::new(Duration::from_millis(20), 512);
        buf.set_passthrough(true);
        b.iter(|| {
            buf.push(black_box(b'a'));
            if buf.len() >= 400 {
                black_box(buf.take());
            }
        });
    });
}

fn bench_adaptive_interval_update(c: &mut Criterion) {
    c.bench_function("adaptive_interval_update", |b| {
        let mut buf = InputBuffer::new(Duration::from_millis(20), 512);
        buf.set_adaptive(true);
        b.iter(|| {
            buf.set_adaptive_interval(black_box(Duration::from_millis(150)));
        });
    });
}

fn bench_contains_enter_raw(c: &mut Criterion) {
    // Simulate scanning a 1KB output chunk for the alt-screen escape sequence.
    let chunk: Vec<u8> = (0u8..=255u8).cycle().take(1024).collect();
    c.bench_function("contains_enter_raw_1kb", |b| {
        b.iter(|| {
            const ENTER: &[u8] = b"\x1b[?1049h";
            let found = black_box(&chunk).windows(ENTER.len()).any(|w| w == ENTER);
            black_box(found);
        });
    });
}

fn bench_metrics_record_flush(c: &mut Criterion) {
    c.bench_function("metrics_record_flush", |b| {
        let mut m = SessionMetrics::new(32);
        b.iter(|| {
            m.record_flush(black_box(10));
        });
    });
}

criterion_group!(
    benches,
    bench_push_single_byte,
    bench_push_1000_bytes,
    bench_take,
    bench_passthrough_overhead,
    bench_adaptive_interval_update,
    bench_contains_enter_raw,
    bench_metrics_record_flush,
);
criterion_main!(benches);
