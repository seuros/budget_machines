//! Error-budget accounting for one compliance period at a time.

use crate::{Counts, MILLI, Objective, Refusal};

/// `consumed_milli` of an account that spent exactly its allowance.
const SPENT: u64 = MILLI as u64;

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
    period: u64,
    counts: Counts,
    base: u64,
    allowance: u64,
    standing: Standing,
    consumed_milli: Option<u64>,
}

impl Account {
    /// An account rebuilt from its fields, or `None` when no ledger could
    /// have produced it. The objective is not part of an account, so only
    /// what holds for every objective is checked: `base` covers the observed
    /// total; `allowance` does not exceed `base`, since a tolerated fraction
    /// is at most one; `standing` is the one `counts` and
    /// `allowance` give; `consumed_milli` is absent exactly when `base` is
    /// zero, is zero when nothing was bad, at most `1000` within the
    /// allowance and at least `1000` beyond it.
    #[must_use]
    pub const fn new(
        period: u64,
        counts: Counts,
        base: u64,
        allowance: u64,
        standing: Standing,
        consumed_milli: Option<u64>,
    ) -> Option<Self> {
        let bad = counts.bad();
        let shaped = base >= counts.total() && allowance <= base;
        let consumed = match consumed_milli {
            None => base == 0,
            Some(milli) => base > 0 && (bad > 0 || milli == 0),
        };
        let consistent = match standing {
            Standing::NoData => counts.total() == 0,
            Standing::Within { remaining } => {
                counts.total() > 0
                    && bad <= allowance
                    && remaining == allowance - bad
                    && matches!(consumed_milli, None | Some(0..=SPENT))
            }
            Standing::Exhausted { overspent } => {
                counts.total() > 0
                    && bad > allowance
                    && overspent == bad - allowance
                    && matches!(consumed_milli, Some(SPENT..))
            }
        };
        if !(shaped && consumed && consistent) {
            return None;
        }
        Some(Self {
            period,
            counts,
            base,
            allowance,
            standing,
            consumed_milli,
        })
    }

    /// Period the counts belong to.
    #[must_use]
    pub const fn period(&self) -> u64 {
        self.period
    }

    /// Good and total events observed in the period.
    #[must_use]
    pub const fn counts(&self) -> Counts {
        self.counts
    }

    /// Total the allowance was computed against: observed, or the plan.
    #[must_use]
    pub const fn base(&self) -> u64 {
        self.base
    }

    /// Whole bad events tolerated among `base` events.
    #[must_use]
    pub const fn allowance(&self) -> u64 {
        self.allowance
    }

    /// Verdict; `NoData` whenever nothing was observed.
    #[must_use]
    pub const fn standing(&self) -> Standing {
        self.standing
    }

    /// Budget consumed in thousandths (`1000` is all of it), rounded down and
    /// saturating. `None` when `base` is zero.
    #[must_use]
    pub const fn consumed_milli(&self) -> Option<u64> {
        self.consumed_milli
    }
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
