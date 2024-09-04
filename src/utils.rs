use byte_unit::{AdjustedByte, Byte, UnitType};
use rayon::iter::{FromParallelIterator, IndexedParallelIterator, ParallelIterator};
use std::io::{stderr, IsTerminal};
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{info, info_span, Span};
use tracing_indicatif::span_ext::IndicatifSpanExt;

pub fn display_bytes(size: u64) -> AdjustedByte {
    Byte::from(size).get_appropriate_unit(UnitType::Binary)
}

const PBAR_TEMPLATE: &str = "{span_child_prefix} {msg} {percent}% {wide_bar} {per_sec} [{human_pos}/{human_len}]";

const SPINNER_TEMPLATE: &str = "{span_child_prefix} {spinner} {msg} {per_sec}";

fn setup_span_bar(span: &Span, size: usize, message: &'static str) -> Span {
    span.pb_set_message(message);
    span.pb_set_style(&indicatif::ProgressStyle::default_bar().template(PBAR_TEMPLATE).unwrap());
    span.pb_set_length(size as u64);
    Span::current()
}

fn setup_span_spinner(span: &Span, message: &'static str) -> Span {
    span.pb_set_message(message);
    span.pb_set_style(
        &indicatif::ProgressStyle::default_spinner()
            .template(SPINNER_TEMPLATE)
            .unwrap(),
    );
    Span::current()
}

#[inline(always)]
fn tick(span: &Span, total: usize, current: usize, is_term: bool) -> usize {
    if is_term {
        span.pb_inc(1);
    } else {
        let ten_percent = total / 10;
        if current % ten_percent == 0 {
            info!("{}%", (current as f64 / total as f64 * 100.0) as u64);
        }
    }
    total + 1
}

pub fn progress_parallel_collect<V: FromParallelIterator<T>, T: Send>(
    message: &'static str,
    iterator: impl IndexedParallelIterator<Item=anyhow::Result<T>>,
) -> anyhow::Result<V> {
    let span = info_span!("task");
    let entered = span.enter();
    let total = iterator.len();
    let span = setup_span_bar(&span, total, message);
    let is_term = stderr().is_terminal();
    let current_counter = AtomicUsize::new(0);

    iterator
        .inspect(move |_| {
            let current = current_counter.fetch_add(1, Ordering::Relaxed);
            tick(&span, total, current, is_term);
            let _ = entered;
        })
        .collect::<anyhow::Result<V>>()
}

pub fn progress_iter<T>(
    message: &'static str,
    iterator: impl ExactSizeIterator<Item=T>,
) -> impl ExactSizeIterator<Item=T> {
    let total = iterator.len();
    let span = info_span!("task");
    let entered = span.enter();
    let span = setup_span_bar(&span, total, message);
    let is_term = stderr().is_terminal();
    let mut current = 0;
    iterator.inspect(move |_| {
        current = tick(&span, total, current, is_term);
        let _ = entered;
    })
}

pub fn spinner_iter<T>(message: &'static str, iterator: impl Iterator<Item=T>) -> impl Iterator<Item=T> {
    let span = info_span!("task");
    let entered = span.enter();
    let span = setup_span_spinner(&span, message);

    iterator.inspect(move |_| {
        span.pb_inc(1);
        let _ = entered;
    })
}
