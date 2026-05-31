//! Criterion bench — StatusDetector 吞吐(性能门禁)
//!
//! 测三件事:
//!   1. 通用 idle/running 推断(无 agent 规则)
//!   2. claude 规则下大段 stdout
//!   3. OSC 133 parser
//!
//! 跑:`cargo bench -p vibeterm-status`
//! CI smoke:`cargo bench -p vibeterm-status -- --quick`

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use vibeterm_status::StatusDetector;

fn bench_generic_running(c: &mut Criterion) {
    let chunk = "Lorem ipsum dolor sit amet, consectetur adipiscing elit.\n".repeat(100);
    let mut group = c.benchmark_group("status_detector/generic");
    group.throughput(Throughput::Bytes(chunk.len() as u64));
    group.bench_function("feed_chunk_no_match", |b| {
        b.iter(|| {
            let mut d = StatusDetector::new("zsh");
            d.feed(black_box(chunk.as_bytes()));
        });
    });
    group.finish();
}

fn bench_claude_rules(c: &mut Criterion) {
    // 模拟 Claude 大段输出,最后一行是 waiting_input
    let mut bulk = "Thinking...\n".repeat(50);
    bulk.push_str("Do you want to proceed? (y/n) ");
    let mut group = c.benchmark_group("status_detector/claude");
    group.throughput(Throughput::Bytes(bulk.len() as u64));
    group.bench_function("feed_with_match", |b| {
        b.iter(|| {
            let mut d = StatusDetector::new("claude");
            d.feed(black_box(bulk.as_bytes()));
        });
    });
    group.finish();
}

fn bench_osc133(c: &mut Criterion) {
    let chunk = b"\x1b]133;A\x1b\\some output\x1b]133;D;0\x1b\\";
    let mut group = c.benchmark_group("status_detector/osc133");
    group.throughput(Throughput::Bytes(chunk.len() as u64));
    group.bench_function("parse_prompt_and_finish", |b| {
        b.iter(|| {
            let mut d = StatusDetector::new("zsh");
            d.feed(black_box(chunk));
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_generic_running,
    bench_claude_rules,
    bench_osc133
);
criterion_main!(benches);
