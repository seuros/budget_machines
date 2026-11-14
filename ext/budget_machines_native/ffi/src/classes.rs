//! The `Data` classes, resolved once at load, and the two rules the binding
//! applies to every value it reads from Ruby or keeps in a receiver.
//!
//! A value read back must be an instance of exactly the gem's class: a
//! subclass constant leaks past the seal and `Data#initialize` can be bound
//! directly, so `is_a?` is not enough. Its fields are read straight from the
//! struct (no reader runs) and rebuilt through the core constructor, which
//! `Marshal.load` skipped.
use std::cell::RefCell;
use std::sync::OnceLock;

use magnus::rb_sys::AsRawValue;
use magnus::typed_data::Obj;
use magnus::value::Opaque;
use magnus::{Error, RClass, RModule, RStruct, Ruby, TryConvert, TypedData, Value, prelude::*};

/// Every `Data` class the binding checks or builds.
pub struct Classes {
    pub counts: Opaque<RClass>,
    pub objective: Opaque<RClass>,
    pub within: Opaque<RClass>,
    pub exhausted: Opaque<RClass>,
    pub account: Opaque<RClass>,
    pub sample: Opaque<RClass>,
    pub windows: Opaque<RClass>,
    pub rule: Opaque<RClass>,
    pub decision: Opaque<RClass>,
    pub verdict: Opaque<RClass>,
}

static CLASSES: OnceLock<Classes> = OnceLock::new();

fn setup_error(ruby: &Ruby) -> Error {
    Error::new(
        ruby.exception_runtime_error(),
        "load budget_machines, not the extension alone",
    )
}

/// Resolve the classes `budget_machines.rb` defined before requiring the
/// extension.
pub fn resolve(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = |scope: RModule, name: &str| -> Result<Opaque<RClass>, Error> {
        let class: RClass = scope.const_get(name).map_err(|_| setup_error(ruby))?;
        // Pinned: a static holds it, and `GC.compact` must not move it.
        ruby.gc_register_mark_object(class);
        Ok(Opaque::from(class))
    };
    let standing: RModule = module
        .const_get("Standing")
        .map_err(|_| setup_error(ruby))?;
    let sample: RModule = module.const_get("Sample").map_err(|_| setup_error(ruby))?;
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
    CLASSES.set(classes).map_err(|_| setup_error(ruby))
}

/// The classes resolved at load.
pub fn classes(ruby: &Ruby) -> Result<&'static Classes, Error> {
    CLASSES.get().ok_or_else(|| setup_error(ruby))
}

/// Whether `value` is an instance of exactly `class`, never a subclass:
/// pointer equality, so no `==` or `instance_of?` the caller could redefine
/// runs. A singleton class opened on the instance itself does not change
/// what it is.
pub fn is(ruby: &Ruby, value: Value, class: Opaque<RClass>) -> bool {
    let (actual, class) = (value.class(), ruby.get_inner(class));
    if actual.as_raw() == class.as_raw() {
        return true;
    }
    let own = RStruct::from_value(value)
        .and_then(|held| held.singleton_class().ok())
        .is_some_and(|own| own.as_raw() == actual.as_raw());
    own && actual
        .superclass()
        .is_ok_and(|parent| parent.as_raw() == class.as_raw())
}

/// `value` as a struct, if it is an instance of exactly `class`; `TypeError`
/// with `message` otherwise.
pub fn exact(
    ruby: &Ruby,
    value: Value,
    class: Opaque<RClass>,
    message: &'static str,
) -> Result<RStruct, Error> {
    if !is(ruby, value, class) {
        return Err(crate::type_error(ruby, message));
    }
    RStruct::from_value(value).ok_or_else(|| crate::type_error(ruby, message))
}

/// Field `index` of a struct, converted; read without calling its reader.
pub fn field<T: TryConvert>(value: RStruct, index: usize) -> Result<T, Error> {
    value.aref(index)
}

/// Clone-then-keep: `decide` runs on a copy of the core and `build` turns its
/// answer into Ruby, with no borrow held. The copy is stored only when the
/// receiver is still unfrozen (`FrozenError`) and unchanged (`RuntimeError`,
/// a re-entrant call changed it); otherwise nothing is applied.
pub fn keep<T, C, A>(
    ruby: &Ruby,
    this: Obj<T>,
    core: &RefCell<C>,
    decide: impl FnOnce(&mut C) -> A,
    build: impl FnOnce(A) -> Result<Value, Error>,
) -> Result<Value, Error>
where
    T: TypedData,
    C: Clone + PartialEq,
{
    this.check_frozen()?;
    let before = core.borrow().clone();
    let mut after = before.clone();
    let answer = build(decide(&mut after))?;
    this.check_frozen()?;
    if *core.borrow() != before {
        return Err(Error::new(
            ruby.exception_runtime_error(),
            "the receiver changed while the answer was being built; the call was not applied",
        ));
    }
    *core.borrow_mut() = after;
    Ok(answer)
}
