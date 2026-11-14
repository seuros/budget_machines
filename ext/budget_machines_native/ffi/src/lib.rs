//! Thin binding to the error-budget ledger and burn-rate monitor.
mod classes;
mod init;
mod ledger;
mod monitor;
mod values;

use budget_machines::{Counts, Objective, Refusal};
use magnus::{Error, Ruby, Value, prelude::*};
use matryona_machines::ruby::{Symbols, exact_fields, integer, symbol};

use crate::classes::classes;

/// Rules one monitor can hold.
const CAPACITY: usize = 8;

matryona_machines::symbols! {
    /// The core's refusals as Symbols: the snake case of each variant.
    Refusals for Refusal as "a refusal" {
        StalePeriod => "stale_period",
        Overflow => "overflow",
        BelowObserved => "below_observed",
        Full => "full",
        Shape => "shape",
        TimeRegression => "time_regression",
    }
}

fn type_error(ruby: &Ruby, message: &'static str) -> Error {
    Error::new(ruby.exception_type_error(), message)
}

fn range_error(ruby: &Ruby, message: &'static str) -> Error {
    Error::new(ruby.exception_range_error(), message)
}

/// A `BudgetMachines::Objective`, rebuilt through `Objective::new`.
fn objective(ruby: &Ruby, value: Value) -> Result<Objective, Error> {
    let class = classes(ruby)?.objective.get(ruby);
    let [good, per] =
        exact_fields::<2>(ruby, value, class, "expected a BudgetMachines::Objective")?;
    let good = integer(good, "good")?;
    ratio(ruby, good, integer(per, "per")?)
}

fn ratio(ruby: &Ruby, good: u32, per: u32) -> Result<Objective, Error> {
    Objective::new(good, per).ok_or_else(|| {
        range_error(
            ruby,
            "objective needs good < per: a 100% target has no error budget",
        )
    })
}

/// A `BudgetMachines::Counts`, rebuilt through `Counts::new`.
fn counts(ruby: &Ruby, value: Value) -> Result<Counts, Error> {
    let class = classes(ruby)?.counts.get(ruby);
    let [good, total] = exact_fields::<2>(ruby, value, class, "expected a BudgetMachines::Counts")?;
    let good = integer(good, "good")?;
    pair(ruby, good, integer(total, "total")?)
}

fn pair(ruby: &Ruby, good: u64, total: u64) -> Result<Counts, Error> {
    Counts::new(good, total).ok_or_else(|| range_error(ruby, "good exceeds total"))
}

fn counts_value(ruby: &Ruby, counts: Counts) -> Result<Value, Error> {
    let class = classes(ruby)?.counts.get(ruby);
    class.funcall("new", (counts.good(), counts.total()))
}

fn objective_value(ruby: &Ruby, objective: Objective) -> Result<Value, Error> {
    let class = classes(ruby)?.objective.get(ruby);
    class.funcall("new", (objective.good(), objective.per()))
}

fn refusal(ruby: &Ruby, refused: Refusal) -> Value {
    Refusals::symbol(ruby, refused).as_value()
}

/// `:ok` for an honoured call that answers nothing else, or the refusal.
fn done(ruby: &Ruby, outcome: Result<(), Refusal>) -> Value {
    outcome.map_or_else(
        |refused| refusal(ruby, refused),
        |()| symbol(ruby, "ok").as_value(),
    )
}

fn allowance(ruby: &Ruby, target: Value, total: Value) -> Result<u64, Error> {
    let total = integer(total, "total")?;
    Ok(objective(ruby, target)?.allowance(total))
}

fn burn_milli_of(
    ruby: &Ruby,
    target: Value,
    bad: Value,
    total: Value,
) -> Result<Option<u64>, Error> {
    let (bad, total) = (integer(bad, "bad")?, integer(total, "total")?);
    Ok(objective(ruby, target)?.burn_milli_of(bad, total))
}

fn burn_milli(ruby: &Ruby, target: Value, window: Value) -> Result<Option<u64>, Error> {
    Ok(objective(ruby, target)?.burn_milli(counts(ruby, window)?))
}

fn burns_at_least(
    ruby: &Ruby,
    target: Value,
    window: Value,
    threshold_milli: Value,
) -> Result<bool, Error> {
    let threshold_milli = integer(threshold_milli, "threshold_milli")?;
    let (target, window) = (objective(ruby, target)?, counts(ruby, window)?);
    Ok(target.burns_at_least(window, threshold_milli))
}
