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

#[inline(always)]
fn setup_span_bar(span: &Span, size: usize, message: &'static str) -> Span {
    span.pb_set_message(message);
    span.pb_set_style(&indicatif::ProgressStyle::default_bar().template(PBAR_TEMPLATE).unwrap());
    span.pb_set_length(size as u64);
    Span::current()
}

#[inline(always)]
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
pub fn progress_parallel_collect<V: FromParallelIterator<T>, T: Send>(
    message: &'static str,
    iterator: impl IndexedParallelIterator<Item = anyhow::Result<T>>,
) -> anyhow::Result<V> {
    let span = info_span!("task");
    let entered = span.enter();
    let span = setup_span_bar(&span, iterator.len(), message);
    iterator
        .inspect(move |_| {
            span.pb_inc(1);
            let _ = entered;
        })
        .collect::<anyhow::Result<V>>()
}

#[inline(always)]
pub fn progress_iter<T>(
    message: &'static str,
    iterator: impl ExactSizeIterator<Item = T>,
) -> impl ExactSizeIterator<Item = T> {
    let size = iterator.len();
    let span = info_span!("task");
    let entered = span.enter();
    let span = setup_span_bar(&span, size, message);
    iterator.inspect(move |_| {
        span.pb_inc(1);
        let _ = entered;
    })
}

#[inline(always)]
pub fn spinner_iter<T>(message: &'static str, iterator: impl Iterator<Item = T>) -> impl Iterator<Item = T> {
    let span = info_span!("task");
    let entered = span.enter();
    let span = setup_span_spinner(&span, message);
    iterator.inspect(move |_| {
        span.pb_inc(1);
        let _ = entered;
    })
}
