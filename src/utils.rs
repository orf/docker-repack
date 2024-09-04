use std::io::{stderr, IsTerminal};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use byte_unit::{AdjustedByte, Byte, UnitType};
use rayon::iter::{FromParallelIterator, IndexedParallelIterator, ParallelIterator};
use tracing::{info_span, Span};
use tracing_indicatif::span_ext::IndicatifSpanExt;

pub fn display_bytes(size: u64) -> AdjustedByte {
    Byte::from(size).get_appropriate_unit(UnitType::Binary)
}

// pub struct SpanProgress<'a, T: ExactSizeIterator> {
//     span: Span,
//     guard: Entered<'a>,
//     iterator: T,
// }
//
// impl<T: ExactSizeIterator> Iterator for SpanProgress<'_, T> {
//     type Item = T::Item;
//
//     fn next(&mut self) -> Option<Self::Item> {
//         self.iterator.next()
//     }
// }

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
fn tick(span: &Span, total: usize, is_term: bool) -> usize {
    if is_term {
        span.pb_inc(1);
    } else {
        span.pb_inc(1);
    }
    total + 1
}

pub fn progress_parallel_collect<V: FromParallelIterator<T>, T: Send>(
    message: &'static str,
    iterator: impl IndexedParallelIterator<Item = anyhow::Result<T>>,
) -> anyhow::Result<V> {
    let span = info_span!("task");
    let entered = span.enter();
    let span = setup_span_bar(&span, iterator.len(), message);
    let is_term = stderr().is_terminal();
    let total_counter = AtomicUsize::new(0);

    iterator
        .inspect(move |_| {
            let total = total_counter.fetch_add(1, Ordering::Relaxed);
            tick(&span, total, is_term);
            let _ = entered;
        })
        .collect::<anyhow::Result<V>>()
}

pub fn progress_iter<T>(
    message: &'static str,
    iterator: impl ExactSizeIterator<Item = T>,
) -> impl ExactSizeIterator<Item = T> {
    let size = iterator.len();
    let span = info_span!("task");
    let entered = span.enter();
    let span = setup_span_bar(&span, size, message);
    let is_term = stderr().is_terminal();
    let mut total = 0;
    iterator.inspect(move |_| {
        total = tick(&span, total, is_term);
        let _ = entered;
    })
}

pub fn spinner_iter<T>(message: &'static str, iterator: impl Iterator<Item = T>) -> impl Iterator<Item = T> {
    let span = info_span!("task");
    let entered = span.enter();
    let span = setup_span_spinner(&span, message);
    let is_term = stderr().is_terminal();
    let mut total = 0;

    iterator.inspect(move |_| {
        total = tick(&span, total, is_term);
        let _ = entered;
    })
}
