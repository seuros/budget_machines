//! Thin binding to the error-budget ledger and burn-rate monitor.
mod init;
mod ledger;
mod monitor;

use budget_machines::{Counts, Objective, Refusal};
use magnus::{Error, Integer, RModule, Ruby, Symbol, TryConvert, Value, prelude::*};

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

fn objective(ruby: &Ruby, good: Small, per: Small) -> Result<Objective, Error> {
    Objective::new(good.0, per.0).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "objective needs good < per: a 100% target has no error budget",
        )
    })
}

fn counts(ruby: &Ruby, good: Number, total: Number) -> Result<Counts, Error> {
    Counts::new(good.0, total.0)
        .ok_or_else(|| Error::new(ruby.exception_range_error(), "good exceeds total"))
}

fn refusal(ruby: &Ruby, refused: Refusal) -> Symbol {
    ruby.to_symbol(match refused {
        Refusal::StalePeriod => "stale_period",
        Refusal::Overflow => "overflow",
        Refusal::BelowObserved => "below_observed",
        Refusal::Full => "full",
        Refusal::Shape => "shape",
        Refusal::TimeRegression => "time_regression",
        _ => "refused",
    })
}

/// A value object class defined on the Ruby side with `Data.define`.
fn data_class(ruby: &Ruby, name: &str) -> Result<Value, Error> {
    ruby.class_object()
        .const_get::<_, RModule>("BudgetMachines")?
        .const_get(name)
}

fn burn_milli(
    ruby: &Ruby,
    good: Small,
    per: Small,
    window_good: Number,
    window_total: Number,
) -> Result<Option<u64>, Error> {
    Ok(objective(ruby, good, per)?.burn_milli(counts(ruby, window_good, window_total)?))
}

fn allowance(ruby: &Ruby, good: Small, per: Small, total: Number) -> Result<u64, Error> {
    Ok(objective(ruby, good, per)?.allowance(total.0))
}
