use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ptyx::buffer::InputBuffer;
use std::time::Duration;

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

criterion_group!(
    benches,
    bench_push_single_byte,
    bench_push_1000_bytes,
    bench_take
);
criterion_main!(benches);
