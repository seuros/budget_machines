//! Thin binding to the error-budget ledger and burn-rate monitor.
mod classes;
mod init;
mod ledger;
mod monitor;
mod values;

use std::time::Duration;

use budget_machines::{Counts, Objective, Refusal};
use magnus::{Error, Integer, Ruby, TryConvert, Value, prelude::*};

use crate::classes::{classes, exact, field};

/// Rules one monitor can hold.
const CAPACITY: usize = 8;

#[derive(Clone, Copy)]
struct Number(u64);

impl TryConvert for Number {
    fn try_convert(value: Value) -> Result<Self, Error> {
        if Integer::from_value(value).is_none() {
            let message = "expected an Integer (unsigned 64-bit)";
            return Err(type_error(&Ruby::get_with(value), message));
        }
        u64::try_convert(value).map(Self)
    }
}

/// An instant: Integer nanoseconds in `0..2**64-1`.
#[derive(Clone, Copy)]
struct Nanos(Duration);

impl TryConvert for Nanos {
    fn try_convert(value: Value) -> Result<Self, Error> {
        if Integer::from_value(value).is_none() {
            return Err(type_error(
                &Ruby::get_with(value),
                "expected Integer nanoseconds",
            ));
        }
        u64::try_convert(value).map(|nanos| Self(Duration::from_nanos(nanos)))
    }
}

/// A duration as Integer nanoseconds, exact however large.
fn nanos(ruby: &Ruby, duration: Duration) -> Integer {
    ruby.integer_from_u128(duration.as_nanos())
}

#[derive(Clone, Copy)]
struct Small(u32);

impl TryConvert for Small {
    fn try_convert(value: Value) -> Result<Self, Error> {
        let number = Number::try_convert(value)?;
        u32::try_from(number.0)
            .map(Self)
            .map_err(|_| range_error(&Ruby::get_with(value), "expected an Integer in 0..2**32-1"))
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
    let held = exact(
        ruby,
        value,
        classes(ruby)?.objective,
        "expected a BudgetMachines::Objective",
    )?;
    ratio(ruby, field(held, 0)?, field(held, 1)?)
}

fn ratio(ruby: &Ruby, good: Small, per: Small) -> Result<Objective, Error> {
    Objective::new(good.0, per.0).ok_or_else(|| {
        range_error(
            ruby,
            "objective needs good < per: a 100% target has no error budget",
        )
    })
}

/// A `BudgetMachines::Counts`, rebuilt through `Counts::new`.
fn counts(ruby: &Ruby, value: Value) -> Result<Counts, Error> {
    let held = exact(
        ruby,
        value,
        classes(ruby)?.counts,
        "expected a BudgetMachines::Counts",
    )?;
    pair(ruby, field(held, 0)?, field(held, 1)?)
}

fn pair(ruby: &Ruby, good: Number, total: Number) -> Result<Counts, Error> {
    Counts::new(good.0, total.0).ok_or_else(|| range_error(ruby, "good exceeds total"))
}

fn counts_value(ruby: &Ruby, counts: Counts) -> Result<Value, Error> {
    let class = ruby.get_inner(classes(ruby)?.counts);
    class.funcall("new", (counts.good(), counts.total()))
}

fn objective_value(ruby: &Ruby, objective: Objective) -> Result<Value, Error> {
    let class = ruby.get_inner(classes(ruby)?.objective);
    class.funcall("new", (objective.good(), objective.per()))
}

fn refusal(ruby: &Ruby, refused: Refusal) -> Value {
    ruby.to_symbol(match refused {
        Refusal::StalePeriod => "stale_period",
        Refusal::Overflow => "overflow",
        Refusal::BelowObserved => "below_observed",
        Refusal::Full => "full",
        Refusal::Shape => "shape",
        Refusal::TimeRegression => "time_regression",
    })
    .as_value()
}

/// `:ok` for an honoured call that answers nothing else, or the refusal.
fn done(ruby: &Ruby, outcome: Result<(), Refusal>) -> Value {
    outcome.map_or_else(
        |refused| refusal(ruby, refused),
        |()| ruby.to_symbol("ok").as_value(),
    )
}

fn allowance(ruby: &Ruby, target: Value, total: Number) -> Result<u64, Error> {
    Ok(objective(ruby, target)?.allowance(total.0))
}

fn burn_milli_of(
    ruby: &Ruby,
    target: Value,
    bad: Number,
    total: Number,
) -> Result<Option<u64>, Error> {
    Ok(objective(ruby, target)?.burn_milli_of(bad.0, total.0))
}

fn burn_milli(ruby: &Ruby, target: Value, window: Value) -> Result<Option<u64>, Error> {
    Ok(objective(ruby, target)?.burn_milli(counts(ruby, window)?))
}

fn burns_at_least(
    ruby: &Ruby,
    target: Value,
    window: Value,
    threshold_milli: Small,
) -> Result<bool, Error> {
    let (target, window) = (objective(ruby, target)?, counts(ruby, window)?);
    Ok(target.burns_at_least(window, threshold_milli.0))
}
