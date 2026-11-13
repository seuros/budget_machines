//! Error-budget accounting for one compliance period at a time.

use crate::{Counts, Objective, Refusal};

/// Where the period stands against its allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// No events were observed this period. Not healthy, not exhausted: unknown.
    NoData,
    /// Bad events are within the allowance.
    Within {
        /// Whole bad events still tolerated.
        remaining: u64,
    },
    /// Bad events exceed the allowance.
    Exhausted {
        /// Bad events beyond the allowance.
        overspent: u64,
    },
}

/// A period's budget, computed exactly from its counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Account {
    /// Period the counts belong to.
    pub period: u64,
    /// Good and total events observed in the period.
    pub counts: Counts,
    /// Total the allowance was computed against: observed, or the plan.
    pub base: u64,
    /// Whole bad events tolerated among `base` events.
    pub allowance: u64,
    /// Verdict; `NoData` whenever nothing was observed.
    pub standing: Standing,
    /// Budget consumed in thousandths (`1000` is all of it), rounded down and
    /// saturating. `None` when `base` is zero.
    pub consumed_milli: Option<u64>,
}

/// Accumulates caller-supplied counts for the current compliance period.
///
/// The period is an opaque number the caller advances; the ledger has no clock
/// and never rolls over on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ledger {
    objective: Objective,
    period: u64,
    counts: Counts,
}

impl Ledger {
    /// Empty ledger for `period`.
    #[must_use]
    pub const fn new(objective: Objective, period: u64) -> Self {
        Self {
            objective,
            period,
            counts: Counts { good: 0, total: 0 },
        }
    }

    /// The objective being accounted.
    #[must_use]
    pub const fn objective(&self) -> Objective {
        self.objective
    }

    /// Current period.
    #[must_use]
    pub const fn period(&self) -> u64 {
        self.period
    }

    /// Counts accumulated in the current period.
    #[must_use]
    pub const fn counts(&self) -> Counts {
        self.counts
    }

    /// Add a window's counts to the current period.
    ///
    /// # Errors
    /// [`Refusal::StalePeriod`] when `period` is not the current one, and
    /// [`Refusal::Overflow`] when a sum would not fit in 64 bits.
    pub const fn record(&mut self, period: u64, counts: Counts) -> Result<(), Refusal> {
        if period != self.period {
            return Err(Refusal::StalePeriod);
        }
        let Some(sum) = self.counts.checked_add(counts) else {
            return Err(Refusal::Overflow);
        };
        self.counts = sum;
        Ok(())
    }

    /// Replace the current period's counts with an authoritative reading.
    ///
    /// # Errors
    /// [`Refusal::StalePeriod`] when `period` is not the current one.
    pub const fn replace(&mut self, period: u64, counts: Counts) -> Result<(), Refusal> {
        if period != self.period {
            return Err(Refusal::StalePeriod);
        }
        self.counts = counts;
        Ok(())
    }

    /// Close the current period and start `period` empty, returning the closed
    /// period's final account.
    ///
    /// # Errors
    /// [`Refusal::StalePeriod`] unless `period` is greater than the current one.
    pub fn roll(&mut self, period: u64) -> Result<Account, Refusal> {
        if period <= self.period {
            return Err(Refusal::StalePeriod);
        }
        let closed = self.account();
        self.period = period;
        self.counts = Counts::default();
        Ok(closed)
    }

    /// Budget against the events observed so far.
    #[must_use]
    pub fn account(&self) -> Account {
        self.against(self.counts.total())
    }

    /// Budget against a planned period total, for a budget that should not
    /// shrink to nothing while the period is young.
    ///
    /// # Errors
    /// [`Refusal::BelowObserved`] when the plan is smaller than the total
    /// already observed.
    pub fn account_against(&self, planned_total: u64) -> Result<Account, Refusal> {
        if planned_total < self.counts.total() {
            return Err(Refusal::BelowObserved);
        }
        Ok(self.against(planned_total))
    }

    fn against(&self, base: u64) -> Account {
        let allowance = self.objective.allowance(base);
        let bad = self.counts.bad();
        let standing = if self.counts.total() == 0 {
            Standing::NoData
        } else if bad > allowance {
            Standing::Exhausted {
                overspent: bad - allowance,
            }
        } else {
            Standing::Within {
                remaining: allowance - bad,
            }
        };
        Account {
            period: self.period,
            counts: self.counts,
            base,
            allowance,
            standing,
            consumed_milli: self.objective.burn_milli_of(bad, base),
        }
    }
}
