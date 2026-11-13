//! `BudgetMachines::Monitor`.
use std::cell::RefCell;

use budget_machines::{Change, Decision, OnNoData, Rule, Sample, State, Windows};
use magnus::{Error, Integer, RArray, Ruby, Symbol, Value, prelude::*, typed_data::Obj};

use crate::{
    CAPACITY, Nanos, Number, Small, counts, data, expect, nanos, objective, objective_value,
    refusal,
};

#[magnus::wrap(class = "BudgetMachines::Monitor", free_immediately, size)]
pub struct Monitor {
    core: RefCell<budget_machines::Monitor<CAPACITY>>,
}

const fn state(state: State) -> &'static str {
    match state {
        State::NoData => "no_data",
        State::Quiet => "quiet",
        State::Firing => "firing",
    }
}

fn type_error(ruby: &Ruby, message: &'static str) -> Error {
    Error::new(ruby.exception_type_error(), message)
}

/// A window reading: `:no_data`, or a `Sample::Counts` wrapping a `Counts`.
fn sample(ruby: &Ruby, value: Value) -> Result<Sample, Error> {
    const MESSAGE: &str = "a window reading is :no_data or a BudgetMachines::Sample::Counts";
    if let Some(symbol) = Symbol::from_value(value) {
        return match symbol.name()?.as_ref() {
            "no_data" => Ok(Sample::NoData),
            _ => Err(type_error(ruby, MESSAGE)),
        };
    }
    expect(ruby, value, "Sample::Counts", MESSAGE)?;
    counts(ruby, value.funcall("counts", ())?).map(Sample::Counts)
}

fn windows(ruby: &Ruby, value: Value) -> Result<Vec<Windows>, Error> {
    let rows = RArray::from_value(value)
        .ok_or_else(|| type_error(ruby, "windows must be an Array of Windows"))?;
    let mut parsed = Vec::with_capacity(rows.len());
    for index in 0..rows.len() {
        let row: Value = isize::try_from(index)
            .map_err(|_| type_error(ruby, "too many windows"))
            .and_then(|index| rows.entry(index))?;
        expect(
            ruby,
            row,
            "Windows",
            "each rule needs a BudgetMachines::Windows",
        )?;
        parsed.push(Windows {
            long: sample(ruby, row.funcall("long", ())?)?,
            short: sample(ruby, row.funcall("short", ())?)?,
        });
    }
    Ok(parsed)
}

fn decision(ruby: &Ruby, decision: &Decision) -> Result<Value, Error> {
    let change = match decision.change {
        Change::None => "none",
        Change::Raised => "raised",
        Change::Cleared => "cleared",
    };
    data(
        ruby,
        "Decision",
        (
            ruby.to_symbol(state(decision.state)),
            ruby.to_symbol(change),
            decision.measured,
            decision.long_milli,
            decision.short_milli,
            decision.since.map(|since| nanos(ruby, since)),
        ),
    )
}

fn policy(ruby: &Ruby, value: Value) -> Result<OnNoData, Error> {
    let symbol =
        Symbol::from_value(value).ok_or_else(|| type_error(ruby, "policy must be a Symbol"))?;
    match symbol.name()?.as_ref() {
        "hold" => Ok(OnNoData::Hold),
        "reset" => Ok(OnNoData::Reset),
        "fire" => Ok(OnNoData::Fire),
        _ => Err(Error::new(
            ruby.exception_arg_error(),
            "policy must be :hold, :reset or :fire",
        )),
    }
}

fn rule(ruby: &Ruby, value: Value) -> Result<Rule, Error> {
    expect(ruby, value, "Rule", "expected a BudgetMachines::Rule")?;
    let range = |message| Error::new(ruby.exception_range_error(), message);
    let (fire_milli, clear_milli, severity): (Small, Small, Number) = (
        value.funcall("fire_milli", ())?,
        value.funcall("clear_milli", ())?,
        value.funcall("severity", ())?,
    );
    let severity = u8::try_from(severity.0).map_err(|_| range("severity must be 0..255"))?;
    Rule::new(fire_milli.0, clear_milli.0, severity)
        .ok_or_else(|| range("thresholds need 0 < clear_milli <= fire_milli"))
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
            Err(refused) => refusal(ruby, refused).as_value(),
        })
    }

    pub fn evaluate(
        ruby: &Ruby,
        this: Obj<Self>,
        now: Nanos,
        readings: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let readings = windows(ruby, readings)?;
        let outcome = this.core.borrow_mut().evaluate(now.0, &readings);
        let verdict = match outcome {
            Ok(verdict) => verdict,
            Err(refused) => return Ok(refusal(ruby, refused).as_value()),
        };
        let decisions = ruby.ary_new_capa(verdict.decisions().len());
        for each in verdict.decisions() {
            decisions.push(decision(ruby, each)?)?;
        }
        decisions.freeze();
        data(ruby, "Verdict", (decisions, verdict.worst()))
    }

    pub fn objective(ruby: &Ruby, this: &Self) -> Result<Value, Error> {
        objective_value(ruby, this.core.borrow().objective())
    }

    pub fn rule(ruby: &Ruby, this: &Self, at: Number) -> Result<Option<Value>, Error> {
        let rule = this.core.borrow().rule(index(at));
        rule.map(|rule| {
            data(
                ruby,
                "Rule",
                (rule.fire_milli(), rule.clear_milli(), rule.severity()),
            )
        })
        .transpose()
    }

    pub fn state(ruby: &Ruby, this: &Self, at: Number) -> Option<Symbol> {
        let current = this.core.borrow().state(index(at))?;
        Some(ruby.to_symbol(state(current)))
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
