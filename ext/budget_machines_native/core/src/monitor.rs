//! Multi-window burn-rate alerting with explicit no-data handling.

use core::time::Duration;

use crate::{Counts, Objective, Refusal};

/// One window's reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sample {
    /// The caller has no reading for this window.
    NoData,
    /// Exact counts. Zero events, or fewer than the monitor's minimum, still
    /// count as no data.
    Counts(Counts),
}

/// The long and short window readings one rule is judged on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Windows {
    /// Long window: is the burn significant?
    pub long: Sample,
    /// Short window: is it still happening?
    pub short: Sample,
}

/// What a rule does when either of its windows has no data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnNoData {
    /// Keep the previous state.
    Hold,
    /// Fall back to [`State::NoData`], clearing a firing alert.
    Reset,
    /// Treat silence as an outage and fire.
    Fire,
}

/// A burn-rate rule: fire when both windows burn at `fire_milli` or more, and
/// once firing stay so until either window drops below `clear_milli`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    fire_milli: u32,
    clear_milli: u32,
    severity: u8,
}

impl Rule {
    /// `None` unless `0 < clear_milli <= fire_milli`. Equal thresholds give
    /// the plain stateless rule; a lower clear threshold adds hysteresis.
    #[must_use]
    pub const fn new(fire_milli: u32, clear_milli: u32, severity: u8) -> Option<Self> {
        if clear_milli == 0 || clear_milli > fire_milli {
            return None;
        }
        Some(Self {
            fire_milli,
            clear_milli,
            severity,
        })
    }

    /// Burn rate in thousandths at which the rule fires.
    #[must_use]
    pub const fn fire_milli(self) -> u32 {
        self.fire_milli
    }

    /// Burn rate in thousandths below which a firing rule clears.
    #[must_use]
    pub const fn clear_milli(self) -> u32 {
        self.clear_milli
    }

    /// Caller-defined rank; higher wins [`Verdict::worst`].
    #[must_use]
    pub const fn severity(self) -> u8 {
        self.severity
    }
}

/// A rule's alert state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Never measured, or reset by missing data.
    #[default]
    NoData,
    /// Measured and not alerting.
    Quiet,
    /// Alerting.
    Firing,
}

/// What an evaluation did to a rule's alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The alert neither started nor stopped.
    None,
    /// The alert started firing.
    Raised,
    /// The alert stopped firing.
    Cleared,
}

/// One rule's outcome for one evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    state: State,
    change: Change,
    long_milli: Option<u64>,
    short_milli: Option<u64>,
    since: Option<Duration>,
}

impl Decision {
    /// A decision rebuilt from its fields, or `None` when no evaluation could
    /// have produced it: `Raised` lands in `Firing` and `Cleared` leaves it;
    /// an alert edge has the instant it happened; a decision with both burn
    /// rates was measured, so its state is `Quiet` or `Firing`.
    #[must_use]
    pub const fn new(
        state: State,
        change: Change,
        long_milli: Option<u64>,
        short_milli: Option<u64>,
        since: Option<Duration>,
    ) -> Option<Self> {
        let firing = matches!(state, State::Firing);
        let edge = match change {
            Change::None => true,
            Change::Raised => firing && since.is_some(),
            Change::Cleared => !firing && since.is_some(),
        };
        let measured = long_milli.is_some() && short_milli.is_some();
        if !edge || (measured && matches!(state, State::NoData)) {
            return None;
        }
        Some(Self {
            state,
            change,
            long_milli,
            short_milli,
            since,
        })
    }

    /// State after the evaluation.
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    /// Alert edge, if any.
    #[must_use]
    pub const fn change(&self) -> Change {
        self.change
    }

    /// Whether both windows had data, so both burn rates are known. `false`
    /// means `state` came from the no-data policy, not from a measurement.
    #[must_use]
    pub const fn measured(&self) -> bool {
        self.long_milli.is_some() && self.short_milli.is_some()
    }

    /// Long-window burn rate in thousandths; `None` without data.
    #[must_use]
    pub const fn long_milli(&self) -> Option<u64> {
        self.long_milli
    }

    /// Short-window burn rate in thousandths; `None` without data.
    #[must_use]
    pub const fn short_milli(&self) -> Option<u64> {
        self.short_milli
    }

    /// Instant the rule entered `state`; `None` if it never left its start.
    #[must_use]
    pub const fn since(&self) -> Option<Duration> {
        self.since
    }
}

const UNDECIDED: Decision = Decision {
    state: State::NoData,
    change: Change::None,
    long_milli: None,
    short_milli: None,
    since: None,
};

/// Every rule's decision for one evaluation, in rule order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict<const R: usize> {
    decisions: [Decision; R],
    len: usize,
    worst: Option<usize>,
}

impl<const R: usize> Verdict<R> {
    /// A verdict rebuilt from its parts, or `None` when no evaluation could
    /// have produced it: more than `R` decisions, or a `worst` that is not
    /// the index of a firing decision, or no `worst` while one fires.
    #[must_use]
    pub fn new(decisions: &[Decision], worst: Option<usize>) -> Option<Self> {
        let mut held = [UNDECIDED; R];
        held.get_mut(..decisions.len())?.copy_from_slice(decisions);
        let firing = decisions
            .iter()
            .any(|decision| decision.state == State::Firing);
        let named = worst.map_or(!firing, |index| {
            decisions
                .get(index)
                .is_some_and(|decision| decision.state == State::Firing)
        });
        named.then_some(Self {
            decisions: held,
            len: decisions.len(),
            worst,
        })
    }

    /// One decision per rule.
    #[must_use]
    pub fn decisions(&self) -> &[Decision] {
        self.decisions.get(..self.len).unwrap_or_else(|| {
            debug_assert!(false, "a verdict holds at most R decisions");
            &[]
        })
    }

    /// Index of the firing rule with the highest severity; the earliest rule
    /// wins a tie. `None` when nothing fires.
    #[must_use]
    pub const fn worst(&self) -> Option<usize> {
        self.worst
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    rule: Rule,
    state: State,
    since: Option<Duration>,
}

/// Up to `R` burn-rate rules over one objective.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor<const R: usize> {
    objective: Objective,
    policy: OnNoData,
    min_total: u64,
    slots: [Option<Slot>; R],
    len: usize,
    last: Option<Duration>,
}

impl<const R: usize> Monitor<R> {
    /// Monitor with no rules. A window with fewer than `min_total` events (and
    /// always one with none) is treated as no data.
    #[must_use]
    pub const fn new(objective: Objective, policy: OnNoData, min_total: u64) -> Self {
        Self {
            objective,
            policy,
            min_total,
            slots: [None; R],
            len: 0,
            last: None,
        }
    }

    /// Append a rule, starting in [`State::NoData`], and return its index.
    ///
    /// # Errors
    /// [`Refusal::Full`] when all `R` slots are taken.
    pub fn add_rule(&mut self, rule: Rule) -> Result<usize, Refusal> {
        let index = self.len;
        let slot = self.slots.get_mut(index).ok_or(Refusal::Full)?;
        *slot = Some(Slot {
            rule,
            state: State::NoData,
            since: None,
        });
        self.len += 1;
        Ok(index)
    }

    /// Number of rules.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no rule has been added.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The objective being monitored.
    #[must_use]
    pub const fn objective(&self) -> Objective {
        self.objective
    }

    /// Rule at `index`.
    #[must_use]
    pub fn rule(&self, index: usize) -> Option<Rule> {
        self.slot(index).map(|slot| slot.rule)
    }

    /// Current state of the rule at `index`.
    #[must_use]
    pub fn state(&self, index: usize) -> Option<State> {
        self.slot(index).map(|slot| slot.state)
    }

    /// Instant of the latest accepted evaluation.
    #[must_use]
    pub const fn last(&self) -> Option<Duration> {
        self.last
    }

    /// Index of the firing rule with the highest severity, earliest on a tie.
    #[must_use]
    pub fn worst(&self) -> Option<usize> {
        let mut worst: Option<(usize, u8)> = None;
        for (index, slot) in self.slots.iter().flatten().enumerate() {
            let outranks = worst.is_none_or(|(_, severity)| slot.rule.severity > severity);
            if slot.state == State::Firing && outranks {
                worst = Some((index, slot.rule.severity));
            }
        }
        worst.map(|(index, _)| index)
    }

    /// Judge every rule on its window pair, `windows[i]` for rule `i`.
    ///
    /// `now` is monotonic time since an epoch the caller picks. It orders
    /// evaluations and stamps [`Decision::since`]; the window lengths are the
    /// caller's and never reach the monitor.
    ///
    /// # Errors
    /// [`Refusal::TimeRegression`] when `now` precedes the previous evaluation
    /// and [`Refusal::Shape`] when `windows` does not hold one pair per rule.
    /// Neither changes any alert state.
    pub fn evaluate(&mut self, now: Duration, windows: &[Windows]) -> Result<Verdict<R>, Refusal> {
        if self.last.is_some_and(|last| now < last) {
            return Err(Refusal::TimeRegression);
        }
        if windows.len() != self.len {
            return Err(Refusal::Shape);
        }
        let (objective, policy, floor) = (self.objective, self.policy, self.min_total);
        let mut decisions = [UNDECIDED; R];
        let pairs = self.slots.iter_mut().flatten().zip(windows);
        for ((slot, pair), decision) in pairs.zip(decisions.iter_mut()) {
            *decision = judge(objective, policy, floor, slot, *pair, now);
        }
        self.last = Some(now);
        Ok(Verdict {
            decisions,
            len: self.len,
            worst: self.worst(),
        })
    }

    fn slot(&self, index: usize) -> Option<&Slot> {
        self.slots.get(index).and_then(Option::as_ref)
    }
}

const fn usable(sample: Sample, floor: u64) -> Option<Counts> {
    match sample {
        Sample::Counts(counts) if counts.total() > 0 && counts.total() >= floor => Some(counts),
        Sample::Counts(_) | Sample::NoData => None,
    }
}

fn judge(
    objective: Objective,
    policy: OnNoData,
    floor: u64,
    slot: &mut Slot,
    pair: Windows,
    now: Duration,
) -> Decision {
    let (long, short) = (usable(pair.long, floor), usable(pair.short, floor));
    let was_firing = slot.state == State::Firing;
    let state = if let (Some(long), Some(short)) = (long, short) {
        let bar = if was_firing {
            slot.rule.clear_milli
        } else {
            slot.rule.fire_milli
        };
        if objective.burns_at_least(long, bar) && objective.burns_at_least(short, bar) {
            State::Firing
        } else {
            State::Quiet
        }
    } else {
        match policy {
            OnNoData::Hold => slot.state,
            OnNoData::Reset => State::NoData,
            OnNoData::Fire => State::Firing,
        }
    };
    if state != slot.state {
        slot.state = state;
        slot.since = Some(now);
    }
    let change = match (was_firing, state == State::Firing) {
        (false, true) => Change::Raised,
        (true, false) => Change::Cleared,
        (false, false) | (true, true) => Change::None,
    };
    Decision {
        state,
        change,
        long_milli: long.and_then(|counts| objective.burn_milli(counts)),
        short_milli: short.and_then(|counts| objective.burn_milli(counts)),
        since: slot.since,
    }
}
