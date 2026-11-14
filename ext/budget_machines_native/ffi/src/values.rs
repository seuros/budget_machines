//! The private hooks every `Data` value validates through at construction, so
//! a value built by hand is held to the core's constructors exactly like one
//! the gem returned.
use budget_machines::{Account, Change, Decision, Standing, State, Verdict};
use magnus::{Error, Integer, RArray, Ruby, Symbol, Value, prelude::*};

use crate::classes::{classes, exact, field, is};
use crate::{
    CAPACITY, Nanos, Number, Small, counts, counts_value, monitor, nanos, pair, range_error, ratio,
    type_error,
};

/// What `Account.new` stores, in field order.
type AccountFields = (u64, Value, u64, u64, Value, Option<u64>);

/// What `Decision.new` stores, in field order.
type DecisionFields = (Symbol, Symbol, Option<u64>, Option<u64>, Option<Integer>);

/// The name of a Symbol, or `TypeError`.
pub fn symbol_name(ruby: &Ruby, value: Value, message: &'static str) -> Result<String, Error> {
    let symbol = Symbol::from_value(value).ok_or_else(|| type_error(ruby, message))?;
    Ok(symbol.name()?.into_owned())
}

fn state(ruby: &Ruby, value: Value) -> Result<State, Error> {
    match symbol_name(ruby, value, "a rule state is a Symbol")?.as_str() {
        "no_data" => Ok(State::NoData),
        "quiet" => Ok(State::Quiet),
        "firing" => Ok(State::Firing),
        _ => Err(range_error(
            ruby,
            "a rule state is :no_data, :quiet or :firing",
        )),
    }
}

fn change(ruby: &Ruby, value: Value) -> Result<Change, Error> {
    match symbol_name(ruby, value, "a change is a Symbol")?.as_str() {
        "none" => Ok(Change::None),
        "raised" => Ok(Change::Raised),
        "cleared" => Ok(Change::Cleared),
        _ => Err(range_error(ruby, "a change is :none, :raised or :cleared")),
    }
}

fn standing(ruby: &Ruby, value: Value) -> Result<Standing, Error> {
    const MESSAGE: &str = "a standing is :no_data, a Standing::Within or a Standing::Exhausted";
    if Symbol::from_value(value).is_some() {
        return match symbol_name(ruby, value, MESSAGE)?.as_str() {
            "no_data" => Ok(Standing::NoData),
            _ => Err(range_error(ruby, MESSAGE)),
        };
    }
    let classes = classes(ruby)?;
    if is(ruby, value, classes.within) {
        let remaining: Number = field(exact(ruby, value, classes.within, MESSAGE)?, 0)?;
        return Ok(Standing::Within {
            remaining: remaining.0,
        });
    }
    let overspent: Number = field(exact(ruby, value, classes.exhausted, MESSAGE)?, 0)?;
    Ok(Standing::Exhausted {
        overspent: overspent.0,
    })
}

/// A core `Decision` from Ruby field values, as `Decision::new` judges them.
fn build_decision(
    ruby: &Ruby,
    held_state: Value,
    held_change: Value,
    long_milli: Option<Number>,
    short_milli: Option<Number>,
    since: Option<Nanos>,
) -> Result<Decision, Error> {
    Decision::new(
        state(ruby, held_state)?,
        change(ruby, held_change)?,
        long_milli.map(|milli| milli.0),
        short_milli.map(|milli| milli.0),
        since.map(|since| since.0),
    )
    .ok_or_else(|| range_error(ruby, "no evaluation produces this decision"))
}

/// The fields `Decision.new` takes for a core decision, in order.
pub fn decision_fields(ruby: &Ruby, decision: &Decision) -> DecisionFields {
    (
        ruby.to_symbol(monitor::state_name(decision.state())),
        ruby.to_symbol(monitor::change_name(decision.change())),
        decision.long_milli(),
        decision.short_milli(),
        decision.since().map(|since| nanos(ruby, since)),
    )
}

fn decision(ruby: &Ruby, value: Value) -> Result<Decision, Error> {
    let held = exact(
        ruby,
        value,
        classes(ruby)?.decision,
        "expected a BudgetMachines::Decision",
    )?;
    build_decision(
        ruby,
        field(held, 0)?,
        field(held, 1)?,
        field(held, 2)?,
        field(held, 3)?,
        field(held, 4)?,
    )
}

fn checked_counts(ruby: &Ruby, good: Number, total: Number) -> Result<(u64, u64), Error> {
    pair(ruby, good, total).map(|counts| (counts.good(), counts.total()))
}

fn checked_add(ruby: &Ruby, left: Value, right: Value) -> Result<Option<Value>, Error> {
    let sum = counts(ruby, left)?.checked_add(counts(ruby, right)?);
    sum.map(|sum| counts_value(ruby, sum)).transpose()
}

fn checked_objective(ruby: &Ruby, good: Small, per: Small) -> Result<(u32, u32), Error> {
    ratio(ruby, good, per).map(|objective| (objective.good(), objective.per()))
}

fn checked_rule(
    ruby: &Ruby,
    fire_milli: Small,
    clear_milli: Small,
    severity: Number,
) -> Result<(u32, u32, u8), Error> {
    let rule = monitor::checked_rule(ruby, fire_milli, clear_milli, severity)?;
    Ok((rule.fire_milli(), rule.clear_milli(), rule.severity()))
}

const fn checked_u64(value: Number) -> u64 {
    value.0
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
    period: Number,
    window: Value,
    base: Number,
    allowance: Number,
    held: Value,
    consumed_milli: Option<Number>,
) -> Result<AccountFields, Error> {
    let account = Account::new(
        period.0,
        counts(ruby, window)?,
        base.0,
        allowance.0,
        standing(ruby, held)?,
        consumed_milli.map(|milli| milli.0),
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
    long_milli: Option<Number>,
    short_milli: Option<Number>,
    since: Option<Nanos>,
) -> Result<DecisionFields, Error> {
    let decision = build_decision(
        ruby,
        held_state,
        held_change,
        long_milli,
        short_milli,
        since,
    )?;
    Ok(decision_fields(ruby, &decision))
}

fn checked_verdict(
    ruby: &Ruby,
    decisions: Value,
    worst: Option<Number>,
) -> Result<(RArray, Option<u64>), Error> {
    let rows = RArray::from_value(decisions)
        .ok_or_else(|| type_error(ruby, "decisions must be an Array of Decision"))?;
    let held = ruby.ary_new_capa(rows.len());
    let mut parsed = Vec::with_capacity(rows.len());
    for index in 0..rows.len() {
        let row: Value = rows.entry(isize::try_from(index).unwrap_or(isize::MAX))?;
        parsed.push(decision(ruby, row)?);
        held.push(row)?;
    }
    let worst = worst.map(|index| usize::try_from(index.0).unwrap_or(usize::MAX));
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
    module.define_singleton_method("__u64", function!(checked_u64, 1))?;
    module.define_singleton_method("__sample", function!(checked_sample, 1))?;
    module.define_singleton_method("__windows", function!(checked_windows, 2))?;
    module.define_singleton_method("__account", function!(checked_account, 6))?;
    module.define_singleton_method("__decision", function!(checked_decision, 5))?;
    module.define_singleton_method("__verdict", function!(checked_verdict, 2))?;
    Ok(())
}
