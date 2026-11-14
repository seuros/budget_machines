//! The private hooks every `Data` value validates through at construction, so
//! a value built by hand is held to the core's constructors exactly like one
//! the gem returned.
use std::time::Duration;

use budget_machines::{Account, Decision, Standing, Verdict};
use magnus::{Error, Integer, RArray, Ruby, Symbol, Value, prelude::*};
use matryona_machines::ruby::{
    Symbols, exact_array, exact_fields, integer, nanoseconds, optional, which,
};

use crate::classes::classes;
use crate::monitor::{self, Changes, States};
use crate::{CAPACITY, counts, counts_value, pair, range_error, ratio, type_error};

/// What `Account.new` stores, in field order.
type AccountFields = (u64, Value, u64, u64, Value, Option<u64>);

/// What `Decision.new` stores, in field order.
type DecisionFields = (Symbol, Symbol, Option<u64>, Option<u64>, Option<Integer>);

/// A `Decision`'s optional fields as the core takes them, in field order.
type Marks = (Option<u64>, Option<u64>, Option<Duration>);

fn standing(ruby: &Ruby, value: Value) -> Result<Standing, Error> {
    const MESSAGE: &str = "a standing is :no_data, a Standing::Within or a Standing::Exhausted";
    if let Some(name) = Symbol::from_value(value) {
        return match &*name.name()? {
            "no_data" => Ok(Standing::NoData),
            _ => Err(range_error(ruby, MESSAGE)),
        };
    }
    let classes = classes(ruby)?;
    let held = [classes.within.get(ruby), classes.exhausted.get(ruby)];
    let Some(index) = which(value, &held) else {
        return Err(type_error(ruby, MESSAGE));
    };
    let [field] = exact_fields::<1>(ruby, value, held[index], MESSAGE)?;
    Ok(if index == 0 {
        Standing::Within {
            remaining: integer(field, "remaining")?,
        }
    } else {
        Standing::Exhausted {
            overspent: integer(field, "overspent")?,
        }
    })
}

/// The optional fields of a `Decision`, converted in field order.
fn marks(long_milli: Value, short_milli: Value, since: Value) -> Result<Marks, Error> {
    let long_milli = optional(long_milli, "long_milli")?;
    let short_milli = optional(short_milli, "short_milli")?;
    let since = optional(since, "since")?.map(Duration::from_nanos);
    Ok((long_milli, short_milli, since))
}

/// A core `Decision` from Ruby field values, as `Decision::new` judges them.
/// The optional fields are converted first, then the two Symbols.
fn build_decision(
    ruby: &Ruby,
    held_state: Value,
    held_change: Value,
    (long_milli, short_milli, since): Marks,
) -> Result<Decision, Error> {
    Decision::new(
        States::parse(ruby, held_state)?,
        Changes::parse(ruby, held_change)?,
        long_milli,
        short_milli,
        since,
    )
    .ok_or_else(|| range_error(ruby, "no evaluation produces this decision"))
}

/// The fields `Decision.new` takes for a core decision, in order.
pub fn decision_fields(ruby: &Ruby, decision: &Decision) -> DecisionFields {
    (
        States::symbol(ruby, decision.state()),
        Changes::symbol(ruby, decision.change()),
        decision.long_milli(),
        decision.short_milli(),
        decision.since().map(|since| nanoseconds(ruby, since)),
    )
}

fn decision(ruby: &Ruby, value: Value) -> Result<Decision, Error> {
    let class = classes(ruby)?.decision.get(ruby);
    let [state, change, long_milli, short_milli, since] =
        exact_fields::<5>(ruby, value, class, "expected a BudgetMachines::Decision")?;
    let fields = marks(long_milli, short_milli, since)?;
    build_decision(ruby, state, change, fields)
}

fn checked_counts(ruby: &Ruby, good: Value, total: Value) -> Result<(u64, u64), Error> {
    let good = integer(good, "good")?;
    pair(ruby, good, integer(total, "total")?).map(|counts| (counts.good(), counts.total()))
}

fn checked_add(ruby: &Ruby, left: Value, right: Value) -> Result<Option<Value>, Error> {
    let sum = counts(ruby, left)?.checked_add(counts(ruby, right)?);
    sum.map(|sum| counts_value(ruby, sum)).transpose()
}

fn checked_objective(ruby: &Ruby, good: Value, per: Value) -> Result<(u32, u32), Error> {
    let good = integer(good, "good")?;
    let objective = ratio(ruby, good, integer(per, "per")?)?;
    Ok((objective.good(), objective.per()))
}

fn checked_rule(
    ruby: &Ruby,
    fire_milli: Value,
    clear_milli: Value,
    severity: Value,
) -> Result<(u32, u32, u8), Error> {
    let rule = monitor::checked_rule(ruby, fire_milli, clear_milli, severity)?;
    Ok((rule.fire_milli(), rule.clear_milli(), rule.severity()))
}

/// What `Standing::Within.new` stores.
fn checked_remaining(remaining: Value) -> Result<u64, Error> {
    integer(remaining, "remaining")
}

/// What `Standing::Exhausted.new` stores.
fn checked_overspent(overspent: Value) -> Result<u64, Error> {
    integer(overspent, "overspent")
}

fn checked_sample(ruby: &Ruby, window: Value) -> Result<Value, Error> {
    counts(ruby, window)?;
    Ok(window)
}

fn checked_windows(ruby: &Ruby, long: Value, short: Value) -> Result<(Value, Value), Error> {
    monitor::sample(ruby, long)?;
    monitor::sample(ruby, short)?;
    Ok((long, short))
}

fn checked_account(
    ruby: &Ruby,
    period: Value,
    window: Value,
    base: Value,
    allowance: Value,
    held: Value,
    consumed_milli: Value,
) -> Result<AccountFields, Error> {
    let period = integer(period, "period")?;
    let base = integer(base, "base")?;
    let allowance = integer(allowance, "allowance")?;
    let consumed_milli = optional(consumed_milli, "consumed_milli")?;
    let account = Account::new(
        period,
        counts(ruby, window)?,
        base,
        allowance,
        standing(ruby, held)?,
        consumed_milli,
    )
    .ok_or_else(|| range_error(ruby, "no ledger produces this account"))?;
    Ok((
        account.period(),
        window,
        account.base(),
        account.allowance(),
        held,
        account.consumed_milli(),
    ))
}

fn checked_decision(
    ruby: &Ruby,
    held_state: Value,
    held_change: Value,
    long_milli: Value,
    short_milli: Value,
    since: Value,
) -> Result<DecisionFields, Error> {
    let fields = marks(long_milli, short_milli, since)?;
    let decision = build_decision(ruby, held_state, held_change, fields)?;
    Ok(decision_fields(ruby, &decision))
}

fn checked_verdict(
    ruby: &Ruby,
    decisions: Value,
    worst: Value,
) -> Result<(RArray, Option<u64>), Error> {
    let worst: Option<u64> = optional(worst, "worst")?;
    let rows = exact_array(ruby, decisions, "decisions must be an Array of Decision")?;
    let held = ruby.ary_new_capa(rows.len());
    let mut parsed = Vec::with_capacity(rows.len());
    for index in 0..rows.len() {
        let row: Value = rows.entry(isize::try_from(index).unwrap_or(isize::MAX))?;
        parsed.push(decision(ruby, row)?);
        held.push(row)?;
    }
    let worst = worst.map(|index| usize::try_from(index).unwrap_or(usize::MAX));
    let verdict = Verdict::<CAPACITY>::new(&parsed, worst)
        .ok_or_else(|| range_error(ruby, "no evaluation produces this verdict"))?;
    held.freeze();
    Ok((held, verdict.worst().map(|index| index as u64)))
}

/// Define the private hooks on `BudgetMachines`.
pub fn define(ruby: &Ruby) -> Result<(), Error> {
    use magnus::function;
    let module = ruby.define_module("BudgetMachines")?;
    module.define_singleton_method("__counts", function!(checked_counts, 2))?;
    module.define_singleton_method("__checked_add", function!(checked_add, 2))?;
    module.define_singleton_method("__objective", function!(checked_objective, 2))?;
    module.define_singleton_method("__rule", function!(checked_rule, 3))?;
    module.define_singleton_method("__remaining", function!(checked_remaining, 1))?;
    module.define_singleton_method("__overspent", function!(checked_overspent, 1))?;
    module.define_singleton_method("__sample", function!(checked_sample, 1))?;
    module.define_singleton_method("__windows", function!(checked_windows, 2))?;
    module.define_singleton_method("__account", function!(checked_account, 6))?;
    module.define_singleton_method("__decision", function!(checked_decision, 5))?;
    module.define_singleton_method("__verdict", function!(checked_verdict, 2))?;
    Ok(())
}
