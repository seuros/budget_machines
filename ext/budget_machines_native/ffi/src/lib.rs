//! Thin binding to the error-budget ledger and burn-rate monitor.
mod init;
mod ledger;
mod monitor;

use std::time::Duration;

use budget_machines::{Counts, Objective, Refusal};
use magnus::{Error, Integer, RClass, RModule, Ruby, Symbol, TryConvert, Value, prelude::*};

/// Rules one monitor can hold.
const CAPACITY: usize = 8;

#[derive(Clone, Copy)]
struct Number(u64);

impl TryConvert for Number {
    fn try_convert(value: Value) -> Result<Self, Error> {
        if Integer::from_value(value).is_none() {
            return Err(Error::new(
                Ruby::get_with(value).exception_type_error(),
                "expected an Integer (unsigned 64-bit)",
            ));
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
            return Err(Error::new(
                Ruby::get_with(value).exception_type_error(),
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
        u32::try_from(number.0).map(Self).map_err(|_| {
            Error::new(
                Ruby::get_with(value).exception_range_error(),
                "expected an Integer in 0..2**32-1",
            )
        })
    }
}

/// Insist that `value` is the `Data` class `name`, so its readers are the
/// validated fields.
fn expect(ruby: &Ruby, value: Value, name: &str, message: &'static str) -> Result<(), Error> {
    if value.is_kind_of(data_class(ruby, name)?) {
        return Ok(());
    }
    Err(Error::new(ruby.exception_type_error(), message))
}

/// A `BudgetMachines::Objective`.
fn objective(ruby: &Ruby, value: Value) -> Result<Objective, Error> {
    expect(
        ruby,
        value,
        "Objective",
        "expected a BudgetMachines::Objective",
    )?;
    let (good, per): (Small, Small) = (value.funcall("good", ())?, value.funcall("per", ())?);
    ratio(ruby, good, per)
}

fn ratio(ruby: &Ruby, good: Small, per: Small) -> Result<Objective, Error> {
    Objective::new(good.0, per.0).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "objective needs good < per: a 100% target has no error budget",
        )
    })
}

/// A `BudgetMachines::Counts`.
fn counts(ruby: &Ruby, value: Value) -> Result<Counts, Error> {
    expect(ruby, value, "Counts", "expected a BudgetMachines::Counts")?;
    pair(
        ruby,
        value.funcall("good", ())?,
        value.funcall("total", ())?,
    )
}

fn pair(ruby: &Ruby, good: Number, total: Number) -> Result<Counts, Error> {
    Counts::new(good.0, total.0)
        .ok_or_else(|| Error::new(ruby.exception_range_error(), "good exceeds total"))
}

/// A `Data` value built by keyword-free positional `new`.
fn data<A: magnus::ArgList>(ruby: &Ruby, name: &str, fields: A) -> Result<Value, Error> {
    data_class(ruby, name)?.funcall("new", fields)
}

fn counts_value(ruby: &Ruby, counts: Counts) -> Result<Value, Error> {
    data(ruby, "Counts", (counts.good(), counts.total()))
}

fn objective_value(ruby: &Ruby, objective: Objective) -> Result<Value, Error> {
    data(ruby, "Objective", (objective.good(), objective.per()))
}

fn refusal(ruby: &Ruby, refused: Refusal) -> Symbol {
    ruby.to_symbol(match refused {
        Refusal::StalePeriod => "stale_period",
        Refusal::Overflow => "overflow",
        Refusal::BelowObserved => "below_observed",
        Refusal::Full => "full",
        Refusal::Shape => "shape",
        Refusal::TimeRegression => "time_regression",
    })
}

/// A value object class defined on the Ruby side with `Data.define`; `name`
/// may be nested (`Standing::Within`).
fn data_class(ruby: &Ruby, name: &str) -> Result<RClass, Error> {
    let mut scope: RModule = ruby.class_object().const_get("BudgetMachines")?;
    let mut parts = name.split("::").peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return scope.const_get(part);
        }
        scope = scope.const_get(part)?;
    }
    scope.const_get(name)
}

fn allowance(ruby: &Ruby, good: Small, per: Small, total: Number) -> Result<u64, Error> {
    Ok(ratio(ruby, good, per)?.allowance(total.0))
}

fn burn_milli_of(
    ruby: &Ruby,
    good: Small,
    per: Small,
    bad: Number,
    total: Number,
) -> Result<Option<u64>, Error> {
    Ok(ratio(ruby, good, per)?.burn_milli_of(bad.0, total.0))
}

fn burns_at_least(
    ruby: &Ruby,
    good: Small,
    per: Small,
    window_good: Number,
    window_total: Number,
    threshold_milli: Small,
) -> Result<bool, Error> {
    let counts = pair(ruby, window_good, window_total)?;
    Ok(ratio(ruby, good, per)?.burns_at_least(counts, threshold_milli.0))
}
