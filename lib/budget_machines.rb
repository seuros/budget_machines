# frozen_string_literal: true

require_relative 'budget_machines/version'

# Error-budget accounting and burn-rate alerting. Every value mirrors the Rust
# core's type of the same name, field for field.
module BudgetMachines
  # Every value below validates through the core when built, by hand or by the
  # gem alike: a non-Integer (or other wrong type) raises TypeError, a value
  # the core's constructor rejects raises RangeError.

  # Exact good and total event counts for one window: `Counts::new`, both in
  # 0..2**64-1 with `good <= total`.
  Counts = Data.define(:good, :total) do
    def initialize(good:, total:)
      good, total = BudgetMachines.__send__(:__counts, good, total)
      super
    end

    def bad = total - good

    # Component-wise sum, or nil when either would pass 2**64-1.
    def checked_add(other) = BudgetMachines.__send__(:__checked_add, self, other)
  end

  # Service-level objective as an exact ratio: `good` of every `per` events,
  # both in 0..2**32-1 with `good < per`.
  Objective = Data.define(:good, :per) do
    def initialize(good:, per:)
      good, per = BudgetMachines.__send__(:__objective, good, per)
      super
    end

    def tolerated = per - good

    def allowance(total) = BudgetMachines.__send__(:__allowance, self, total)

    def burn_milli_of(bad, total) = BudgetMachines.__send__(:__burn_milli_of, self, bad, total)

    def burn_milli(counts) = BudgetMachines.__send__(:__burn_milli, self, counts)

    def burns_at_least(counts, threshold_milli)
      BudgetMachines.__send__(:__burns_at_least, self, counts, threshold_milli)
    end
  end

  # Where a period stands against its allowance: :no_data when no events were
  # observed this period, or one of the two values below.
  module Standing
    # Bad events are within the allowance; `remaining` (0..2**64-1) are still tolerated.
    Within = Data.define(:remaining) do
      def initialize(remaining:)
        remaining = BudgetMachines.__send__(:__remaining, remaining)
        super
      end
    end

    # Bad events exceed the allowance by `overspent` (0..2**64-1).
    Exhausted = Data.define(:overspent) do
      def initialize(overspent:)
        overspent = BudgetMachines.__send__(:__overspent, overspent)
        super
      end
    end
  end

  # One compliance period's budget: `Account::new`, which refuses fields no
  # ledger could produce (see the README).
  Account = Data.define(:period, :counts, :base, :allowance, :standing, :consumed_milli) do
    def initialize(period:, counts:, base:, allowance:, standing:, consumed_milli:)
      period, counts, base, allowance, standing, consumed_milli =
        BudgetMachines.__send__(:__account, period, counts, base, allowance, standing, consumed_milli)
      super
    end
  end

  # One window's reading: :no_data when the caller has none, or a
  # Sample::Counts carrying exact counts.
  module Sample
    # Exact counts for the window.
    Counts = Data.define(:counts) do
      def initialize(counts:)
        counts = BudgetMachines.__send__(:__sample, counts)
        super
      end
    end
  end

  # The long and short window readings one rule is judged on. Each is a
  # Sample: :no_data or a Sample::Counts.
  Windows = Data.define(:long, :short) do
    def initialize(long:, short:)
      long, short = BudgetMachines.__send__(:__windows, long, short)
      super
    end
  end

  # A burn-rate rule: fire at `fire_milli`, clear below `clear_milli`, with
  # `0 < clear_milli <= fire_milli` (both 0..2**32-1) and `severity` in 0..255.
  Rule = Data.define(:fire_milli, :clear_milli, :severity) do
    def initialize(fire_milli:, clear_milli:, severity:)
      fire_milli, clear_milli, severity = BudgetMachines.__send__(:__rule, fire_milli, clear_milli, severity)
      super
    end
  end

  # One rule's outcome. `state` is :no_data, :quiet or :firing; `change` is
  # :none, :raised or :cleared. `since` is the instant the rule entered
  # `state`, in Integer nanoseconds as passed to `Monitor#evaluate`, or nil if
  # it never left its start. `Decision::new` refuses fields no evaluation could
  # produce.
  Decision = Data.define(:state, :change, :long_milli, :short_milli, :since) do
    def initialize(state:, change:, long_milli:, short_milli:, since:)
      state, change, long_milli, short_milli, since =
        BudgetMachines.__send__(:__decision, state, change, long_milli, short_milli, since)
      super
    end

    # Both windows had data, so both burn rates are known; false when the
    # no-data policy decided `state`.
    def measured? = !long_milli.nil? && !short_milli.nil?
  end

  # Every rule's decision for one evaluation, in rule order (a frozen Array of
  # at most CAPACITY), and the index of the firing rule with the highest
  # severity (nil when nothing fires). `Verdict::new` refuses a `worst` that
  # does not name a firing decision, or nil while one fires.
  Verdict = Data.define(:decisions, :worst) do
    def initialize(decisions:, worst:)
      decisions, worst = BudgetMachines.__send__(:__verdict, decisions, worst)
      super
    end
  end

  # Sealed: no bare allocation or subclass can skip the validation above.
  # Sealing stops accidents, not reflection (a subclass constant is bound
  # before `inherited` raises, `Marshal.load` skips `initialize`), so the
  # binding also checks every value it reads for its exact class and rebuilds
  # it through the core.
  [
    Counts, Objective, Standing::Within, Standing::Exhausted, Account, Sample::Counts, Windows, Rule,
    Decision, Verdict
  ].each do |value|
    value.singleton_class.undef_method(:allocate)
    value.define_singleton_method(:inherited) { |_| raise TypeError, "#{value} cannot be subclassed" }
  end

  # Native is required: no algorithm fallback. It resolves the classes above
  # once, so they are defined first.
  require 'budget_machines_native/budget_machines_native'

  HOOKS = %i[
    __allowance __burn_milli_of __burn_milli __burns_at_least __counts __checked_add __objective
    __rule __remaining __overspent __sample __windows __account __decision __verdict
  ].freeze
  private_class_method(*HOOKS)
  private_constant :HOOKS
end
