//! `BudgetMachines::Ledger`.
use std::cell::RefCell;

use budget_machines::{Account, Standing};
use magnus::{Error, Ruby, Value, prelude::*, typed_data::Obj};
use matryona_machines::ruby::{SymbolNames, integer, keep};

use crate::classes::classes;
use crate::values::Standings;
use crate::{counts, counts_value, done, objective, objective_value, refusal};

#[magnus::wrap(class = "BudgetMachines::Ledger", free_immediately, size)]
pub struct Ledger {
    core: RefCell<budget_machines::Ledger>,
}

/// A `BudgetMachines::Account` built from a core account, through `new`.
fn account(ruby: &Ruby, account: Account) -> Result<Value, Error> {
    let classes = classes(ruby)?;
    let standing = match account.standing() {
        Standing::NoData => Standings::symbol(ruby, &Standing::NoData).as_value(),
        Standing::Within { remaining } => classes.within.get(ruby).funcall("new", (remaining,))?,
        Standing::Exhausted { overspent } => {
            classes.exhausted.get(ruby).funcall("new", (overspent,))?
        }
    };
    classes.account.get(ruby).funcall(
        "new",
        (
            account.period(),
            counts_value(ruby, account.counts())?,
            account.base(),
            account.allowance(),
            standing,
            account.consumed_milli(),
        ),
    )
}

impl Ledger {
    pub fn new(ruby: &Ruby, target: Value, period: Value) -> Result<Self, Error> {
        let period = integer(period, "period")?;
        let core = budget_machines::Ledger::new(objective(ruby, target)?, period);
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    pub fn period(&self) -> u64 {
        self.core.borrow().period()
    }

    pub fn objective(ruby: &Ruby, this: &Self) -> Result<Value, Error> {
        let held = this.core.borrow().objective();
        objective_value(ruby, held)
    }

    pub fn counts(ruby: &Ruby, this: &Self) -> Result<Value, Error> {
        let held = this.core.borrow().counts();
        counts_value(ruby, held)
    }

    pub fn record(
        ruby: &Ruby,
        this: Obj<Self>,
        period: Value,
        window: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let period = integer(period, "period")?;
        let counts = counts(ruby, window)?;
        let outcome = this.core.borrow_mut().record(period, counts);
        Ok(done(ruby, outcome))
    }

    pub fn replace(
        ruby: &Ruby,
        this: Obj<Self>,
        period: Value,
        reading: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let period = integer(period, "period")?;
        let counts = counts(ruby, reading)?;
        let outcome = this.core.borrow_mut().replace(period, counts);
        Ok(done(ruby, outcome))
    }

    pub fn roll(ruby: &Ruby, this: Obj<Self>, period: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let period = integer(period, "period")?;
        keep(
            ruby,
            this,
            &this.core,
            |core| core.roll(period),
            |outcome| match outcome {
                Ok(closed) => account(ruby, closed),
                Err(refused) => Ok(refusal(ruby, refused)),
            },
        )
    }

    pub fn account(ruby: &Ruby, this: &Self) -> Result<Value, Error> {
        let current = this.core.borrow().account();
        account(ruby, current)
    }

    pub fn account_against(ruby: &Ruby, this: &Self, planned: Value) -> Result<Value, Error> {
        let planned = integer(planned, "planned")?;
        let outcome = this.core.borrow().account_against(planned);
        match outcome {
            Ok(planned) => account(ruby, planned),
            Err(refused) => Ok(refusal(ruby, refused)),
        }
    }
}
