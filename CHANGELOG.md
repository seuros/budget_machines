# Changelog

## [0.4.0](https://github.com/seuros/budget_machines/compare/budget_machines/v0.3.0...budget_machines/v0.4.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* build the binding on matryona-machines 0.2; read every value from the struct; compiler-checked state, change, policy and refusal symbols

### Features

* build the binding on matryona-machines 0.2; read every value from the struct; compiler-checked state, change, policy and refusal symbols ([9fbe6ac](https://github.com/seuros/budget_machines/commit/9fbe6ac909a740ad330633d27ad9128d41745449))

## [0.3.0](https://github.com/seuros/budget_machines/compare/budget_machines/v0.2.0...budget_machines/v0.3.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* seal and validate every value, private Account/Decision fields, derive Decision#measured?, clone-then-keep roll/evaluate

### Features

* seal and validate every value, private Account/Decision fields, derive Decision#measured?, clone-then-keep roll/evaluate ([8ccb894](https://github.com/seuros/budget_machines/commit/8ccb894896018e4b531361f5e2a1f321cf1a13cd))

## [0.2.0](https://github.com/seuros/budget_machines/compare/budget_machines/v0.1.2...budget_machines/v0.2.0) (2026-11-13)


### ⚠ BREAKING CHANGES

* see CONVENTIONS: one exhaustive Refusal per crate, Result from every mutator, honoured idempotent repeats, Ruby mirrors the core. No compatibility aliases. Ruby gains Objective, Counts, Rule, Windows, Verdict, Standing and Sample values; Monitor arguments are required; times are Integer nanoseconds.

### Features

* Refusal without alias, Duration evaluation instants, Ruby mirrors the core types ([0f2792d](https://github.com/seuros/budget_machines/commit/0f2792d88b01d6f7f331f1383aa5c779010f7a24))

## [0.1.2](https://github.com/seuros/budget_machines/compare/budget_machines/v0.1.1...budget_machines/v0.1.2) (2026-11-13)


### Features

* rename Refused to Refusal, keep deprecated alias ([cca679d](https://github.com/seuros/budget_machines/commit/cca679ddb53a8a45d626d141965cd255e45d6b7c))

## [0.1.1](https://github.com/seuros/budget_machines/compare/budget_machines-v0.1.0...budget_machines/v0.1.1) (2026-11-12)


### Features

* initial extraction ([8a0d71a](https://github.com/seuros/budget_machines/commit/8a0d71a7a80ccb3c2fda733ed1aaf0623e88c4c5))
