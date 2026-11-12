//! `BudgetMachines::Monitor`.
use std::cell::RefCell;

use budget_machines::{Change, Decision, OnNoData, Rule, Sample, State, Windows};
use magnus::{
    Error, RArray, RHash, Ruby, Symbol, Value,
    prelude::*,
    scan_args::{get_kwargs, scan_args},
    typed_data::Obj,
};

use crate::{CAPACITY, Number, Small, counts, data_class, objective, refusal};

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

fn sample(ruby: &Ruby, value: Value) -> Result<Sample, Error> {
    if value.is_nil() {
        return Ok(Sample::NoData);
    }
    let pair = RArray::from_value(value)
        .filter(|pair| pair.len() == 2)
        .ok_or_else(|| type_error(ruby, "a window is nil or [good, total]"))?;
    let (good, total): (Number, Number) = (pair.entry(0)?, pair.entry(1)?);
    counts(ruby, good, total).map(Sample::Counts)
}

fn windows(ruby: &Ruby, value: Value) -> Result<Vec<Windows>, Error> {
    let rows = RArray::from_value(value)
        .ok_or_else(|| type_error(ruby, "windows must be an Array of [long, short] pairs"))?;
    let mut parsed = Vec::with_capacity(rows.len());
    for index in 0..rows.len() {
        let row = isize::try_from(index)
            .ok()
            .and_then(|index| rows.entry::<Value>(index).ok())
            .and_then(RArray::from_value)
            .filter(|row| row.len() == 2)
            .ok_or_else(|| type_error(ruby, "each rule needs a [long, short] pair"))?;
        parsed.push(Windows {
            long: sample(ruby, row.entry(0)?)?,
            short: sample(ruby, row.entry(1)?)?,
        });
    }
    Ok(parsed)
}

fn decision(ruby: &Ruby, class: Value, decision: &Decision) -> Result<Value, Error> {
    let change = match decision.change {
        Change::None => None,
        Change::Raised => Some(ruby.to_symbol("raised")),
        Change::Cleared => Some(ruby.to_symbol("cleared")),
    };
    class.funcall(
        "new",
        (
            ruby.to_symbol(state(decision.state)),
            change,
            decision.measured,
            decision.long_milli,
            decision.short_milli,
            decision.since,
        ),
    )
}

impl Monitor {
    pub fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(Small, Small), (), (), (), RHash, ()>(args)?;
        let (good, per) = args.required;
        let (policy, min_total) = get_kwargs::<_, (), (Option<Value>, Option<Number>), ()>(
            args.keywords,
            &[],
            &["on_no_data", "min_total"],
        )?
        .optional;
        let policy = match policy {
            None => OnNoData::Hold,
            Some(value) => {
                let symbol = Symbol::from_value(value)
                    .ok_or_else(|| type_error(ruby, "on_no_data must be a Symbol"))?;
                match symbol.name()?.as_ref() {
                    "hold" => OnNoData::Hold,
                    "reset" => OnNoData::Reset,
                    "fire" => OnNoData::Fire,
                    _ => {
                        return Err(Error::new(
                            ruby.exception_arg_error(),
                            "on_no_data must be :hold, :reset or :fire",
                        ));
                    }
                }
            }
        };
        let floor = min_total.map_or(0, |number| number.0);
        let core = budget_machines::Monitor::new(objective(ruby, good, per)?, policy, floor);
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    pub fn add_rule(
        ruby: &Ruby,
        this: Obj<Self>,
        fire_milli: Small,
        clear_milli: Small,
        severity: Number,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let range = |message| Error::new(ruby.exception_range_error(), message);
        let severity = u8::try_from(severity.0).map_err(|_| range("severity must be 0..255"))?;
        let rule = Rule::new(fire_milli.0, clear_milli.0, severity)
            .ok_or_else(|| range("thresholds need 0 < clear_milli <= fire_milli"))?;
        let outcome = this.core.borrow_mut().add_rule(rule);
        Ok(match outcome {
            Ok(index) => ruby.integer_from_u64(index as u64).as_value(),
            Err(refused) => refusal(ruby, refused).as_value(),
        })
    }

    pub fn evaluate(
        ruby: &Ruby,
        this: Obj<Self>,
        now: Number,
        readings: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let readings = windows(ruby, readings)?;
        let class = data_class(ruby, "Decision")?;
        let outcome = this.core.borrow_mut().evaluate(now.0, &readings);
        let verdict = match outcome {
            Ok(verdict) => verdict,
            Err(refused) => return Ok(refusal(ruby, refused).as_value()),
        };
        let decisions = ruby.ary_new_capa(verdict.decisions().len());
        for each in verdict.decisions() {
            decisions.push(decision(ruby, class, each)?)?;
        }
        Ok(decisions.as_value())
    }

    pub fn states(ruby: &Ruby, this: &Self) -> Result<RArray, Error> {
        let core = this.core.borrow();
        let states = ruby.ary_new_capa(core.len());
        for current in (0..core.len()).filter_map(|index| core.state(index)) {
            states.push(ruby.to_symbol(state(current)))?;
        }
        Ok(states)
    }

    pub fn worst(&self) -> Option<usize> {
        self.core.borrow().worst()
    }

    pub fn last(&self) -> Option<u64> {
        self.core.borrow().last()
    }

    pub fn size(&self) -> usize {
        self.core.borrow().len()
    }

    pub const fn capacity() -> usize {
        CAPACITY
    }
}
