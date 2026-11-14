//! `BudgetMachines::Monitor`.
use std::cell::RefCell;

use budget_machines::{Change, OnNoData, Rule, Sample, State, Verdict, Windows};
use magnus::{Error, Integer, RArray, Ruby, Symbol, Value, prelude::*, typed_data::Obj};

use crate::classes::{classes, exact, field, keep};
use crate::{
    CAPACITY, Nanos, Number, Small, counts, nanos, objective, objective_value, range_error,
    refusal, type_error,
    values::{decision_fields, symbol_name},
};

#[magnus::wrap(class = "BudgetMachines::Monitor", free_immediately, size)]
pub struct Monitor {
    core: RefCell<budget_machines::Monitor<CAPACITY>>,
}

pub const fn state_name(state: State) -> &'static str {
    match state {
        State::NoData => "no_data",
        State::Quiet => "quiet",
        State::Firing => "firing",
    }
}

pub const fn change_name(change: Change) -> &'static str {
    match change {
        Change::None => "none",
        Change::Raised => "raised",
        Change::Cleared => "cleared",
    }
}

/// A window reading: `:no_data`, or a `Sample::Counts` wrapping a `Counts`.
pub fn sample(ruby: &Ruby, value: Value) -> Result<Sample, Error> {
    const MESSAGE: &str = "a window reading is :no_data or a BudgetMachines::Sample::Counts";
    if Symbol::from_value(value).is_some() {
        return match symbol_name(ruby, value, MESSAGE)?.as_str() {
            "no_data" => Ok(Sample::NoData),
            _ => Err(range_error(ruby, MESSAGE)),
        };
    }
    let held = exact(ruby, value, classes(ruby)?.sample, MESSAGE)?;
    counts(ruby, field(held, 0)?).map(Sample::Counts)
}

fn windows(ruby: &Ruby, value: Value) -> Result<Vec<Windows>, Error> {
    let rows = RArray::from_value(value)
        .ok_or_else(|| type_error(ruby, "windows must be an Array of Windows"))?;
    let mut parsed = Vec::with_capacity(rows.len());
    for index in 0..rows.len() {
        let row: Value = isize::try_from(index)
            .map_err(|_| type_error(ruby, "too many windows"))
            .and_then(|index| rows.entry(index))?;
        let held = exact(
            ruby,
            row,
            classes(ruby)?.windows,
            "each rule needs a BudgetMachines::Windows",
        )?;
        parsed.push(Windows {
            long: sample(ruby, field(held, 0)?)?,
            short: sample(ruby, field(held, 1)?)?,
        });
    }
    Ok(parsed)
}

fn policy(ruby: &Ruby, value: Value) -> Result<OnNoData, Error> {
    match symbol_name(ruby, value, "policy must be a Symbol")?.as_str() {
        "hold" => Ok(OnNoData::Hold),
        "reset" => Ok(OnNoData::Reset),
        "fire" => Ok(OnNoData::Fire),
        _ => Err(range_error(ruby, "policy must be :hold, :reset or :fire")),
    }
}

pub fn checked_rule(
    ruby: &Ruby,
    fire_milli: Small,
    clear_milli: Small,
    severity: Number,
) -> Result<Rule, Error> {
    let severity =
        u8::try_from(severity.0).map_err(|_| range_error(ruby, "severity must be 0..255"))?;
    Rule::new(fire_milli.0, clear_milli.0, severity)
        .ok_or_else(|| range_error(ruby, "thresholds need 0 < clear_milli <= fire_milli"))
}

fn rule(ruby: &Ruby, value: Value) -> Result<Rule, Error> {
    let held = exact(
        ruby,
        value,
        classes(ruby)?.rule,
        "expected a BudgetMachines::Rule",
    )?;
    checked_rule(ruby, field(held, 0)?, field(held, 1)?, field(held, 2)?)
}

/// A `BudgetMachines::Verdict` built from a core verdict, through `new`.
fn verdict_value(ruby: &Ruby, verdict: &Verdict<CAPACITY>) -> Result<Value, Error> {
    let classes = classes(ruby)?;
    let decision = ruby.get_inner(classes.decision);
    let decisions = ruby.ary_new_capa(verdict.decisions().len());
    for each in verdict.decisions() {
        decisions.push(decision.funcall::<_, _, Value>("new", decision_fields(ruby, each))?)?;
    }
    ruby.get_inner(classes.verdict)
        .funcall("new", (decisions, verdict.worst()))
}

/// A rule index. One that does not fit in `usize` names no rule.
fn index(rule: Number) -> usize {
    usize::try_from(rule.0).unwrap_or(usize::MAX)
}

impl Monitor {
    pub fn new(
        ruby: &Ruby,
        target: Value,
        on_no_data: Value,
        min_total: Number,
    ) -> Result<Self, Error> {
        let core = budget_machines::Monitor::new(
            objective(ruby, target)?,
            policy(ruby, on_no_data)?,
            min_total.0,
        );
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    pub fn add_rule(ruby: &Ruby, this: Obj<Self>, added: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let rule = rule(ruby, added)?;
        let outcome = this.core.borrow_mut().add_rule(rule);
        Ok(match outcome {
            Ok(index) => ruby.integer_from_u64(index as u64).as_value(),
            Err(refused) => refusal(ruby, refused),
        })
    }

    pub fn evaluate(
        ruby: &Ruby,
        this: Obj<Self>,
        now: Nanos,
        readings: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        // Everything that can raise happens before the core records `now`.
        let readings = windows(ruby, readings)?;
        keep(
            ruby,
            this,
            &this.core,
            |core| core.evaluate(now.0, &readings),
            |outcome| match outcome {
                Ok(verdict) => verdict_value(ruby, &verdict),
                Err(refused) => Ok(refusal(ruby, refused)),
            },
        )
    }

    pub fn objective(ruby: &Ruby, this: &Self) -> Result<Value, Error> {
        let held = this.core.borrow().objective();
        objective_value(ruby, held)
    }

    pub fn rule(ruby: &Ruby, this: &Self, at: Number) -> Result<Option<Value>, Error> {
        let rule = this.core.borrow().rule(index(at));
        let class = ruby.get_inner(classes(ruby)?.rule);
        rule.map(|rule| {
            class.funcall(
                "new",
                (rule.fire_milli(), rule.clear_milli(), rule.severity()),
            )
        })
        .transpose()
    }

    pub fn state(ruby: &Ruby, this: &Self, at: Number) -> Option<Symbol> {
        let current = this.core.borrow().state(index(at))?;
        Some(ruby.to_symbol(state_name(current)))
    }

    pub fn worst(&self) -> Option<usize> {
        self.core.borrow().worst()
    }

    pub fn last(ruby: &Ruby, this: &Self) -> Option<Integer> {
        let last = this.core.borrow().last()?;
        Some(nanos(ruby, last))
    }

    pub fn size(&self) -> usize {
        self.core.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.core.borrow().is_empty()
    }

    pub const fn capacity() -> usize {
        CAPACITY
    }
}
