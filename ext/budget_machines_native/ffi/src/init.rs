//! Private native entry point.
use crate::{CAPACITY, allowance, burn_milli, ledger::Ledger, monitor::Monitor};
use magnus::{Error, Ruby, function, method, prelude::*};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("BudgetMachines")?;
    module.const_set("CAPACITY", CAPACITY)?;
    module.const_set("MILLI", budget_machines::MILLI)?;
    module.define_singleton_method("burn_milli", function!(burn_milli, 4))?;
    module.define_singleton_method("allowance", function!(allowance, 3))?;

    let class = module.define_class("Ledger", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Ledger::new, 3))?;
    class.define_method("period", method!(Ledger::period, 0))?;
    class.define_method("good", method!(Ledger::good, 0))?;
    class.define_method("total", method!(Ledger::total, 0))?;
    class.define_method("record", method!(Ledger::record, 3))?;
    class.define_method("replace", method!(Ledger::replace, 3))?;
    class.define_method("roll", method!(Ledger::roll, 1))?;
    class.define_method("account", method!(Ledger::account, 0))?;
    class.define_method("account_against", method!(Ledger::account_against, 1))?;

    let class = module.define_class("Monitor", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.define_singleton_method("new", function!(Monitor::new, -1))?;
    class.define_method("add_rule", method!(Monitor::add_rule, 3))?;
    class.define_method("evaluate", method!(Monitor::evaluate, 2))?;
    class.define_method("states", method!(Monitor::states, 0))?;
    class.define_method("worst", method!(Monitor::worst, 0))?;
    class.define_method("last", method!(Monitor::last, 0))?;
    class.define_method("size", method!(Monitor::size, 0))?;
    class.define_method("capacity", function!(Monitor::capacity, 0))?;
    Ok(())
}
