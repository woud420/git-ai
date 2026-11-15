//! Time-bucketing for the activity-over-time chart.

use crate::metrics::local_stats::types::{BucketGranularity, BucketStats};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};
use std::collections::HashMap;

pub(super) fn ts_to_local(ts: u32) -> DateTime<Local> {
    Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .unwrap_or_else(Local::now)
}

/// Produce the display label for a bucket whose "anchor" date (Monday for
/// weekly, the day itself for daily) is `date`.
fn bucket_label(date: NaiveDate, granularity: BucketGranularity) -> String {
    match granularity {
        BucketGranularity::Daily => date.format("%b %d").to_string(),
        BucketGranularity::Weekly => {
            let sunday = date + chrono::Duration::days(6);
            format!("{} – {}", date.format("%b %d"), sunday.format("%b %d"))
        }
    }
}

/// Sort key of the bucket containing `dt`; `fill_buckets` derives labels.
pub(super) fn bucket_key(dt: &DateTime<Local>, granularity: BucketGranularity) -> i64 {
    match granularity {
        BucketGranularity::Daily => dt.date_naive().num_days_from_ce() as i64,
        BucketGranularity::Weekly => {
            // ISO week: key on Monday of the week.
            let weekday = dt.weekday().num_days_from_monday() as i64;
            let monday = dt.date_naive() - chrono::Duration::days(weekday);
            monday.num_days_from_ce() as i64
        }
    }
}

/// Per-bucket accumulator for the activity-over-time chart.
#[derive(Debug, Default, Clone)]
pub(super) struct BucketAccum {
    pub(super) ai_lines: u32,
    pub(super) commit_count: u32,
    pub(super) diff_added: u32,
    pub(super) attributed: u32,
}

/// Fill gaps between `since_ts` and today so charts have contiguous buckets.
pub(super) fn fill_buckets(
    mut data_map: HashMap<i64, BucketAccum>,
    since_ts: u32,
    granularity: BucketGranularity,
) -> Vec<BucketStats> {
    let now = Local::now();
    if since_ts == 0 && data_map.is_empty() {
        return Vec::new();
    }
    let since_date = if since_ts == 0 {
        let earliest_order = data_map.keys().copied().min();
        earliest_order
            .and_then(|order| NaiveDate::from_num_days_from_ce_opt(order.try_into().ok()?))
            .unwrap_or_else(|| now.date_naive())
    } else {
        ts_to_local(since_ts).date_naive()
    };

    let make = |label: String, accum: BucketAccum| BucketStats {
        label,
        ai_lines: accum.ai_lines,
        commit_count: accum.commit_count,
        diff_added_lines: accum.diff_added,
        attributed_lines: accum.attributed,
    };

    // Generate all expected bucket keys between since and now.
    let mut result = Vec::new();
    match granularity {
        BucketGranularity::Daily => {
            let mut day = since_date;
            let today = now.date_naive();
            while day <= today {
                let order = day.num_days_from_ce() as i64;
                result.push(make(
                    bucket_label(day, granularity),
                    data_map.remove(&order).unwrap_or_default(),
                ));
                day = day.succ_opt().unwrap_or(today);
            }
        }
        BucketGranularity::Weekly => {
            let weekday = since_date.weekday().num_days_from_monday() as i64;
            let mut monday: NaiveDate = since_date - chrono::Duration::days(weekday);
            let today = now.date_naive();
            while monday <= today {
                let order = monday.num_days_from_ce() as i64;
                result.push(make(
                    bucket_label(monday, granularity),
                    data_map.remove(&order).unwrap_or_default(),
                ));
                monday = monday
                    .checked_add_signed(chrono::Duration::weeks(1))
                    .unwrap_or(today);
            }
        }
    }

    result
}
