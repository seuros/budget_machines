//! `BudgetMachines::Ledger`.
use std::cell::RefCell;

use budget_machines::{Account, Standing};
use magnus::{Error, Ruby, Value, prelude::*, typed_data::Obj};

use crate::{Number, Small, counts, data_class, objective, refusal};

#[magnus::wrap(class = "BudgetMachines::Ledger", free_immediately, size)]
pub struct Ledger {
    core: RefCell<budget_machines::Ledger>,
}

fn account(ruby: &Ruby, account: Account) -> Result<Value, Error> {
    let (standing, remaining, overspent) = match account.standing {
        Standing::NoData => ("no_data", account.allowance, 0),
        Standing::Within { remaining } => ("within", remaining, 0),
        Standing::Exhausted { overspent } => ("exhausted", 0, overspent),
    };
    data_class(ruby, "Account")?.funcall(
        "new",
        (
            account.period,
            account.counts.good(),
            account.counts.total(),
            account.base,
            account.allowance,
            ruby.to_symbol(standing),
            remaining,
            overspent,
            account.consumed_milli,
        ),
    )
}

impl Ledger {
    pub fn new(ruby: &Ruby, good: Small, per: Small, period: Number) -> Result<Self, Error> {
        let core = budget_machines::Ledger::new(objective(ruby, good, per)?, period.0);
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    pub fn period(&self) -> u64 {
        self.core.borrow().period()
    }

    pub fn good(&self) -> u64 {
        self.core.borrow().counts().good()
    }

    pub fn total(&self) -> u64 {
        self.core.borrow().counts().total()
    }

    pub fn record(
        ruby: &Ruby,
        this: Obj<Self>,
        period: Number,
        good: Number,
        total: Number,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let counts = counts(ruby, good, total)?;
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
        good: Number,
        total: Number,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let counts = counts(ruby, good, total)?;
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
