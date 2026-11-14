//! The `Data` classes, resolved once at load and pinned.
//!
//! A value read back must be an instance of exactly the gem's class
//! (`matryona_machines::ruby::exact_struct`): a subclass constant leaks past
//! the seal and `Data#initialize` can be bound directly, so `is_a?` is not
//! enough. Its fields are read straight from the struct (no reader runs) and
//! rebuilt through the core constructor, which `Marshal.load` skipped.
use magnus::{Error, RModule, Ruby, prelude::*};
use matryona_machines::ruby::{Class, Registry};

/// Every `Data` class the binding checks or builds.
pub struct Classes {
    pub counts: Class,
    pub objective: Class,
    pub within: Class,
    pub exhausted: Class,
    pub account: Class,
    pub sample: Class,
    pub windows: Class,
    pub rule: Class,
    pub decision: Class,
    pub verdict: Class,
}

static CLASSES: Registry<Classes> = Registry::new("budget_machines");

/// Resolve the classes `budget_machines.rb` defined before requiring the
/// extension.
pub fn resolve(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let standing: RModule = module.const_get("Standing")?;
    let sample: RModule = module.const_get("Sample")?;
    let class = |scope, name| Class::resolve(ruby, scope, name);
    let classes = Classes {
        counts: class(module, "Counts")?,
        objective: class(module, "Objective")?,
        within: class(standing, "Within")?,
        exhausted: class(standing, "Exhausted")?,
        account: class(module, "Account")?,
        sample: class(sample, "Counts")?,
        windows: class(module, "Windows")?,
        rule: class(module, "Rule")?,
        decision: class(module, "Decision")?,
        verdict: class(module, "Verdict")?,
    };
    CLASSES.set(ruby, classes).map(drop)
}

/// The classes resolved at load.
pub fn classes(ruby: &Ruby) -> Result<&'static Classes, Error> {
    CLASSES.get(ruby)
}
