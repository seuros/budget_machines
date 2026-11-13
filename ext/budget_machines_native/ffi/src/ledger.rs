//! `BudgetMachines::Ledger`.
use std::cell::RefCell;

use budget_machines::{Account, Standing};
use magnus::{Error, Ruby, Value, prelude::*, typed_data::Obj};

use crate::{Number, counts, counts_value, data, objective, objective_value, refusal};

#[magnus::wrap(class = "BudgetMachines::Ledger", free_immediately, size)]
pub struct Ledger {
    core: RefCell<budget_machines::Ledger>,
}

fn account(ruby: &Ruby, account: Account) -> Result<Value, Error> {
    let standing = match account.standing {
        Standing::NoData => ruby.to_symbol("no_data").as_value(),
        Standing::Within { remaining } => data(ruby, "Standing::Within", (remaining,))?,
        Standing::Exhausted { overspent } => data(ruby, "Standing::Exhausted", (overspent,))?,
    };
    data(
        ruby,
        "Account",
        (
            account.period,
            counts_value(ruby, account.counts)?,
            account.base,
            account.allowance,
            standing,
            account.consumed_milli,
        ),
    )
}

impl Ledger {
    pub fn new(ruby: &Ruby, target: Value, period: Number) -> Result<Self, Error> {
        let core = budget_machines::Ledger::new(objective(ruby, target)?, period.0);
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    pub fn period(&self) -> u64 {
        self.core.borrow().period()
    }

    pub fn objective(ruby: &Ruby, this: &Self) -> Result<Value, Error> {
        objective_value(ruby, this.core.borrow().objective())
    }

    pub fn counts(ruby: &Ruby, this: &Self) -> Result<Value, Error> {
        counts_value(ruby, this.core.borrow().counts())
    }

    pub fn record(
        ruby: &Ruby,
        this: Obj<Self>,
        period: Number,
        window: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let counts = counts(ruby, window)?;
        let outcome = this.core.borrow_mut().record(period.0, counts);
        Ok(match outcome {
            Ok(()) => ruby.to_symbol("ok"),
            Err(refused) => refusal(ruby, refused),
        }
        .as_value())
    }

    pub fn replace(
        ruby: &Ruby,
        this: Obj<Self>,
        period: Number,
        reading: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let counts = counts(ruby, reading)?;
        let outcome = this.core.borrow_mut().replace(period.0, counts);
        Ok(match outcome {
            Ok(()) => ruby.to_symbol("ok"),
            Err(refused) => refusal(ruby, refused),
        }
        .as_value())
    }

    pub fn roll(ruby: &Ruby, this: Obj<Self>, period: Number) -> Result<Value, Error> {
        this.check_frozen()?;
        let outcome = this.core.borrow_mut().roll(period.0);
        match outcome {
            Ok(closed) => account(ruby, closed),
            Err(refused) => Ok(refusal(ruby, refused).as_value()),
        }
    }

    pub fn account(ruby: &Ruby, this: &Self) -> Result<Value, Error> {
        let current = this.core.borrow().account();
        account(ruby, current)
    }

    pub fn account_against(ruby: &Ruby, this: &Self, planned: Number) -> Result<Value, Error> {
        let outcome = this.core.borrow().account_against(planned.0);
        match outcome {
            Ok(planned) => account(ruby, planned),
            Err(refused) => Ok(refusal(ruby, refused).as_value()),
        }
    }
}
