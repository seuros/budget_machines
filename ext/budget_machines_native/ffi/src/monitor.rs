//! `BudgetMachines::Monitor`.
use std::cell::RefCell;

use budget_machines::{Change, OnNoData, Rule, Sample, State, Verdict, Windows};
use magnus::{Error, Integer, Ruby, Symbol, Value, prelude::*, typed_data::Obj};
use matryona_machines::ruby::{
    Symbols, duration, exact_array, exact_fields, integer, keep, nanoseconds,
};

use crate::classes::classes;
use crate::{
    CAPACITY, counts, objective, objective_value, range_error, refusal, type_error,
    values::decision_fields,
};

#[magnus::wrap(class = "BudgetMachines::Monitor", free_immediately, size)]
pub struct Monitor {
    core: RefCell<budget_machines::Monitor<CAPACITY>>,
}

matryona_machines::symbols! {
    /// A rule's alert states as Symbols.
    pub States for State as "state" {
        NoData => "no_data",
        Quiet => "quiet",
        Firing => "firing",
    }
}

matryona_machines::symbols! {
    /// What one evaluation changed, as Symbols.
    pub Changes for Change as "change" {
        None => "none",
        Raised => "raised",
        Cleared => "cleared",
    }
}

matryona_machines::symbols! {
    /// The no-data policies as Symbols.
    Policies for OnNoData as "policy" {
        Hold => "hold",
        Reset => "reset",
        Fire => "fire",
    }
}

/// A window reading: `:no_data`, or a `Sample::Counts` wrapping a `Counts`.
pub fn sample(ruby: &Ruby, value: Value) -> Result<Sample, Error> {
    const MESSAGE: &str = "a window reading is :no_data or a BudgetMachines::Sample::Counts";
    if let Some(name) = Symbol::from_value(value) {
        return match &*name.name()? {
            "no_data" => Ok(Sample::NoData),
            _ => Err(range_error(ruby, MESSAGE)),
        };
    }
    let [window] = exact_fields::<1>(ruby, value, classes(ruby)?.sample.get(ruby), MESSAGE)?;
    counts(ruby, window).map(Sample::Counts)
}

fn windows(ruby: &Ruby, value: Value) -> Result<Vec<Windows>, Error> {
    let rows = exact_array(ruby, value, "windows must be an Array of Windows")?;
    let class = classes(ruby)?.windows.get(ruby);
    let mut parsed = Vec::with_capacity(rows.len());
    for index in 0..rows.len() {
        let row: Value = isize::try_from(index)
            .map_err(|_| type_error(ruby, "too many windows"))
            .and_then(|index| rows.entry(index))?;
        let message = "each rule needs a BudgetMachines::Windows";
        let [long, short] = exact_fields::<2>(ruby, row, class, message)?;
        parsed.push(Windows {
            long: sample(ruby, long)?,
            short: sample(ruby, short)?,
        });
    }
    Ok(parsed)
}

/// A rule from its fields, converted in order, through `Rule::new`.
pub fn checked_rule(
    ruby: &Ruby,
    fire_milli: Value,
    clear_milli: Value,
    severity: Value,
) -> Result<Rule, Error> {
    let fire_milli = integer(fire_milli, "fire_milli")?;
    let clear_milli = integer(clear_milli, "clear_milli")?;
    Rule::new(fire_milli, clear_milli, integer(severity, "severity")?)
        .ok_or_else(|| range_error(ruby, "thresholds need 0 < clear_milli <= fire_milli"))
}

fn rule(ruby: &Ruby, value: Value) -> Result<Rule, Error> {
    let class = classes(ruby)?.rule.get(ruby);
    let [fire_milli, clear_milli, severity] =
        exact_fields::<3>(ruby, value, class, "expected a BudgetMachines::Rule")?;
    checked_rule(ruby, fire_milli, clear_milli, severity)
}

/// A `BudgetMachines::Verdict` built from a core verdict, through `new`.
fn verdict_value(ruby: &Ruby, verdict: &Verdict<CAPACITY>) -> Result<Value, Error> {
    let classes = classes(ruby)?;
    let decision = classes.decision.get(ruby);
    let decisions = ruby.ary_new_capa(verdict.decisions().len());
    for each in verdict.decisions() {
        decisions.push(decision.funcall::<_, _, Value>("new", decision_fields(ruby, each))?)?;
    }
    let worst = verdict.worst();
    classes.verdict.get(ruby).funcall("new", (decisions, worst))
}

/// A rule index. One that does not fit in `usize` names no rule.
fn index(rule: Value) -> Result<usize, Error> {
    let rule: u64 = integer(rule, "index")?;
    Ok(usize::try_from(rule).unwrap_or(usize::MAX))
}

impl Monitor {
    pub fn new(
        ruby: &Ruby,
        target: Value,
        on_no_data: Value,
        min_total: Value,
    ) -> Result<Self, Error> {
        let min_total = integer(min_total, "min_total")?;
        let core = budget_machines::Monitor::new(
            objective(ruby, target)?,
            Policies::parse(ruby, on_no_data)?,
            min_total,
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
        now: Value,
        readings: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let now = duration(now, "now")?;
        // Everything that can raise happens before the core records `now`.
        let readings = windows(ruby, readings)?;
        keep(
            ruby,
            this,
            &this.core,
            |core| core.evaluate(now, &readings),
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

    pub fn rule(ruby: &Ruby, this: &Self, at: Value) -> Result<Option<Value>, Error> {
        let rule = this.core.borrow().rule(index(at)?);
        let class = classes(ruby)?.rule.get(ruby);
        rule.map(|rule| {
            class.funcall(
                "new",
                (rule.fire_milli(), rule.clear_milli(), rule.severity()),
            )
        })
        .transpose()
    }

    pub fn state(ruby: &Ruby, this: &Self, at: Value) -> Result<Option<Symbol>, Error> {
        let current = this.core.borrow().state(index(at)?);
        Ok(current.map(|current| States::symbol(ruby, current)))
    }

    pub fn worst(&self) -> Option<usize> {
        self.core.borrow().worst()
    }

    pub fn last(ruby: &Ruby, this: &Self) -> Option<Integer> {
        let last = this.core.borrow().last()?;
        Some(nanoseconds(ruby, last))
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
