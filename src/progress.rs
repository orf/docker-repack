use byte_unit::{AdjustedByte, Byte, UnitType};
use itertools::{Itertools, Position};
use rayon::iter::{FromParallelIterator, IndexedParallelIterator, ParallelIterator};
use std::io::{stderr, IsTerminal};
use std::time::Instant;
use tracing::{info, info_span, Span};
use tracing_indicatif::span_ext::IndicatifSpanExt;

pub fn display_bytes(size: u64) -> AdjustedByte {
    Byte::from(size).get_appropriate_unit(UnitType::Both)
}

const PBAR_TEMPLATE: &str = "{span_child_prefix} {msg} {percent}% {wide_bar} {per_sec} [{human_pos}/{human_len}]";

const SPINNER_TEMPLATE: &str = "{span_child_prefix} {spinner} {msg} {human_pos} - {per_sec}";

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

pub fn progress_parallel_collect<V: FromParallelIterator<T>, T: Send>(
    message: &'static str,
    iterator: impl IndexedParallelIterator<Item = anyhow::Result<T>>,
) -> anyhow::Result<V> {
    let total = iterator.len();
    let span = info_span!("task", items = total);
    let entered = span.enter();
    let span = setup_span_bar(&span, total, message);
    let is_term = stderr().is_terminal();

    if is_term {
        iterator
            .inspect(move |_| {
                span.pb_inc(1);
                let _ = entered;
            })
            .collect()
    } else {
        let start = Instant::now();
        let res = iterator.collect();
        info!("{message} completed in {:#.1?}", start.elapsed());
        let _ = entered;
        res
    }
}

pub fn progress_iter<T>(
    message: &'static str,
    iterator: impl ExactSizeIterator<Item = T>,
) -> impl ExactSizeIterator<Item = T> {
    let total = iterator.len();
    let span = info_span!("task", items = total);
    let entered = span.enter();
    let span = setup_span_bar(&span, total, message);
    let is_term = stderr().is_terminal();
    let start = Instant::now();

    iterator.with_position().map(move |(pos, v)| {
        if is_term {
            span.pb_inc(1);
        } else if pos == Position::Last {
            info!("{message} completed in {:#.1?}", start.elapsed());
        }
        let _ = entered;
        v
    })
}

pub fn spinner_iter<T>(message: &'static str, iterator: impl Iterator<Item = T>) -> impl Iterator<Item = T> {
    let span = info_span!("task");
    let entered = span.enter();
    let span = setup_span_spinner(&span, message);

    iterator.inspect(move |_| {
        span.pb_inc(1);
        let _ = entered;
    })
}
