use crate::{IntervalSc, Symbol, TimestampMs};
use std::cmp::Ordering;

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("Expect interval {expected}, but found {found}.")]
    MismatchedInterval {
        expected: IntervalSc,
        found: IntervalSc,
    },

    // Interval 无法整除
    #[error("Interval {target} cannot be divided by {base}.")]
    UnDivisibleInterval {
        target: IntervalSc,
        base: IntervalSc,
    },

    #[error("Expect symbol {expected}, but found {found}.")]
    MismatchedSymbol { expected: Symbol, found: Symbol },

    #[error(
        "Expect timestamp to be {} {expected}, but found {found}",
        display_ordering(expect_order)
    )]
    UnexpectedTimestamp {
        expect_order: Ordering,
        expected: TimestampMs,
        found: TimestampMs,
    },

    #[error("Unexpect end of stream.")]
    UnexpectedStreamEof,
}

impl DataError {
    pub fn timestamp_should_be_after(expected: TimestampMs, found: TimestampMs) -> Self {
        Self::UnexpectedTimestamp {
            expect_order: Ordering::Greater,
            expected,
            found,
        }
    }

    pub fn timestamp_should_be_before(expected: TimestampMs, found: TimestampMs) -> Self {
        Self::UnexpectedTimestamp {
            expect_order: Ordering::Less,
            expected,
            found,
        }
    }

    pub fn timestamp_should_be_equal(expected: TimestampMs, found: TimestampMs) -> Self {
        Self::UnexpectedTimestamp {
            expect_order: Ordering::Equal,
            expected,
            found,
        }
    }
}

fn display_ordering(order: &Ordering) -> &'static str {
    match order {
        Ordering::Less => "less than",
        Ordering::Equal => "equal to",
        Ordering::Greater => "greater than",
    }
}
