use byte_unit::{Byte, UnitType};
use itertools::Itertools;
use std::fmt::{Display, Formatter};

const HISTOGRAM_STEP_SIZE: usize = 5;
const HISTOGRAM_BUCKETS: usize = (100 / HISTOGRAM_STEP_SIZE) + 1;

#[derive(Debug)]
pub struct ItemHistogramBucket {
    pub quantile: usize,
    pub value: u64,
    pub count: usize,
}

#[derive(Debug)]
pub struct ItemHistogram {
    pub buckets: [ItemHistogramBucket; HISTOGRAM_BUCKETS],
}

impl ItemHistogram {
    pub fn size_histogram(items: impl Iterator<Item = u64>, cumulative: bool) -> Self {
        let mut items = items.collect_vec();
        if cumulative {
            // Cumulative sum all sizes
            items.iter_mut().fold(0, |acc, x| {
                *x += acc;
                *x
            });
        }

        let quantile_range = (0..=100).step_by(HISTOGRAM_STEP_SIZE).map(|v| v as f64);

        let quantiles = quantile_range
            .map(|q| percentile_bounds(&items, q))
            .collect_vec();

        Self {
            buckets: quantiles
                .into_iter()
                .map(|(quantile, value, index)| ItemHistogramBucket {
                    quantile: quantile as usize,
                    value,
                    count: index,
                })
                .collect_vec()
                .try_into()
                .unwrap(),
        }
    }
}

impl Display for ItemHistogram {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Histogram:")?;
        for bucket in &self.buckets {
            write!(f, "  {:3}%: ", bucket.quantile)?;
            if f.alternate() {
                write!(f, "{}", bucket.value)?;
            } else {
                write!(
                    f,
                    "{:#.1}",
                    Byte::from(bucket.value).get_appropriate_unit(UnitType::Decimal)
                )?;
            };
            writeln!(f, " {} items", bucket.count)?;
        }
        Ok(())
    }
}

fn percentile_bounds(data: &[u64], percentile: f64) -> (f64, u64, usize) {
    assert!(!data.is_empty(), "The input data should not be empty.");
    assert!(
        (0.0..=100.0).contains(&percentile),
        "Percentile should be between 0 and 100."
    );

    let len = data.len();
    let rank = percentile / 100.0 * (len - 1) as f64;

    let lower_idx = rank.floor() as usize;

    (percentile, data[lower_idx], lower_idx + 1)
}
