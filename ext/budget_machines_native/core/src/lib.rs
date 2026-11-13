#![no_std]
#![doc = include_str!("../README.md")]

mod ledger;
mod monitor;

pub use ledger::{Account, Ledger, Standing};
pub use monitor::{Change, Decision, Monitor, OnNoData, Rule, Sample, State, Verdict, Windows};

/// Fixed-point scale of every burn rate: `1000` is a burn rate of exactly 1.
pub const MILLI: u32 = 1000;

/// A refused call. The receiver is left exactly as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The period is not the ledger's current one, or does not move forward.
    StalePeriod,
    /// The sum would not fit in 64 bits.
    Overflow,
    /// The planned total is smaller than the total already observed.
    BelowObserved,
    /// Every rule slot is taken.
    Full,
    /// The number of window pairs differs from the number of rules.
    Shape,
    /// The evaluation instant is earlier than the previous one.
    TimeRegression,
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::StalePeriod => "the period is not current or does not move forward",
            Self::Overflow => "the sum does not fit in 64 bits",
            Self::BelowObserved => "the planned total is below the observed total",
            Self::Full => "every rule slot is taken",
            Self::Shape => "the number of window pairs differs from the number of rules",
            Self::TimeRegression => "the instant precedes the previous evaluation",
        })
    }
}

impl core::error::Error for Refusal {}

/// Exact good and total event counts for one window, supplied by the caller.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    good: u64,
    total: u64,
}

impl Counts {
    /// `None` when `good` exceeds `total`.
    #[must_use]
    pub const fn new(good: u64, total: u64) -> Option<Self> {
        if good > total {
            return None;
        }
        Some(Self { good, total })
    }

    /// Events that met the objective.
    #[must_use]
    pub const fn good(self) -> u64 {
        self.good
    }

    /// All valid events.
    #[must_use]
    pub const fn total(self) -> u64 {
        self.total
    }

    /// Events that missed the objective.
    #[must_use]
    pub const fn bad(self) -> u64 {
        self.total - self.good
    }

    /// Component-wise sum, or `None` when either would overflow.
    #[must_use]
    pub const fn checked_add(self, other: Self) -> Option<Self> {
        let (Some(good), Some(total)) = (
            self.good.checked_add(other.good),
            self.total.checked_add(other.total),
        ) else {
            return None;
        };
        Some(Self { good, total })
    }
}

/// Service-level objective as an exact ratio: `good` of every `per` events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Objective {
    good: u32,
    per: u32,
}

impl Objective {
    /// `None` unless `good < per`: a 100% objective has no error budget, so
    /// neither a burn rate nor an allowance is defined for it.
    #[must_use]
    pub const fn new(good: u32, per: u32) -> Option<Self> {
        if good >= per {
            return None;
        }
        Some(Self { good, per })
    }

    /// Target numerator.
    #[must_use]
    pub const fn good(self) -> u32 {
        self.good
    }

    /// Target denominator.
    #[must_use]
    pub const fn per(self) -> u32 {
        self.per
    }

    /// Bad events tolerated per `per` events; never zero.
    #[must_use]
    pub const fn tolerated(self) -> u32 {
        self.per - self.good
    }

    /// Whole bad events the objective tolerates among `total` events:
    /// `floor(total * tolerated / per)`.
    #[must_use]
    pub fn allowance(self, total: u64) -> u64 {
        let exact = u128::from(total) * u128::from(self.tolerated()) / u128::from(self.per);
        // tolerated <= per, so the quotient never exceeds `total`.
        u64::try_from(exact).unwrap_or(u64::MAX)
    }

    /// Burn rate of `bad` events against a base of `total`, in thousandths,
    /// rounded down and saturating at `u64::MAX`. `None` when `total` is zero:
    /// no events is no data, not a burn rate of zero.
    #[must_use]
    pub fn burn_milli_of(self, bad: u64, total: u64) -> Option<u64> {
        if total == 0 {
            return None;
        }
        // bad < 2^64, per < 2^32, MILLI < 2^10: the product fits in 106 bits.
        let spent = u128::from(bad) * u128::from(self.per) * u128::from(MILLI);
        let budget = u128::from(total) * u128::from(self.tolerated());
        Some(u64::try_from(spent / budget).unwrap_or(u64::MAX))
    }

    /// Burn rate of a window in thousandths; see [`Objective::burn_milli_of`].
    #[must_use]
    pub fn burn_milli(self, counts: Counts) -> Option<u64> {
        self.burn_milli_of(counts.bad(), counts.total())
    }

    /// Exact test of `burn rate >= threshold_milli / 1000`, without division.
    /// An empty window burns at no rate, so it never reaches a threshold.
    #[must_use]
    pub fn burns_at_least(self, counts: Counts, threshold_milli: u32) -> bool {
        if counts.total() == 0 {
            return false;
        }
        let spent = u128::from(counts.bad()) * u128::from(self.per) * u128::from(MILLI);
        // 2^32 * 2^64 * 2^32 is the largest product and still fits in 128 bits.
        let bar =
            u128::from(threshold_milli) * u128::from(counts.total()) * u128::from(self.tolerated());
        spent >= bar
    }
}

#[cfg(test)]
mod tests;
