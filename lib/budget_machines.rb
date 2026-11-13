# frozen_string_literal: true

require_relative 'budget_machines/version'
# Native is required: no algorithm fallback.
require 'budget_machines_native/budget_machines_native'

# Error-budget accounting and burn-rate alerting. Every value mirrors the Rust
# core's type of the same name, field for field.
module BudgetMachines
  private_class_method :__allowance, :__burn_milli_of, :__burns_at_least

  # Unsigned integer checks for the value objects.
  module Unsigned
    def self.check(value, bits)
      raise TypeError, "expected an Integer (unsigned #{bits}-bit)" unless value.is_a?(Integer)
      raise RangeError, "expected an Integer in 0..2**#{bits}-1" unless value.between?(0, (1 << bits) - 1)
    end
  end
  private_constant :Unsigned

  # Exact good and total event counts for one window. `good` may not exceed `total`.
  Counts = Data.define(:good, :total) do
    def initialize(good:, total:)
      Unsigned.check(good, 64)
      Unsigned.check(total, 64)
      raise RangeError, 'good exceeds total' if good > total

      super
    end

    def bad = total - good

    # Component-wise sum, or nil when either would pass 2**64-1.
    def checked_add(other)
      good = self.good + other.good
      total = self.total + other.total
      total >= (1 << 64) ? nil : with(good:, total:)
    end
  end

  # Service-level objective as an exact ratio: `good` of every `per` events,
  # with `good < per`.
  Objective = Data.define(:good, :per) do
    def initialize(good:, per:)
      Unsigned.check(good, 32)
      Unsigned.check(per, 32)
      raise RangeError, 'objective needs good < per: a 100% target has no error budget' if good >= per

      super
    end

    def tolerated = per - good

    def allowance(total) = BudgetMachines.send(:__allowance, good, per, total)

    def burn_milli_of(bad, total) = BudgetMachines.send(:__burn_milli_of, good, per, bad, total)

    def burn_milli(counts) = burn_milli_of(counts.bad, counts.total)

    def burns_at_least(counts, threshold_milli)
      BudgetMachines.send(:__burns_at_least, good, per, counts.good, counts.total, threshold_milli)
    end
  end

  # Where a period stands against its allowance: :no_data when no events were
  # observed this period, or one of the two values below.
  module Standing
    # Bad events are within the allowance; `remaining` are still tolerated.
    Within = Data.define(:remaining)
    # Bad events exceed the allowance by `overspent`.
    Exhausted = Data.define(:overspent)
  end

  # One compliance period's budget.
  Account = Data.define(:period, :counts, :base, :allowance, :standing, :consumed_milli) do
    def no_data? = standing == :no_data
    def within? = standing.is_a?(Standing::Within)
    def exhausted? = standing.is_a?(Standing::Exhausted)
  end

  # One window's reading: :no_data when the caller has none, or a
  # Sample::Counts carrying exact counts.
  module Sample
    # Exact counts for the window.
    Counts = Data.define(:counts) do
      def initialize(counts:)
        raise TypeError, 'expected a BudgetMachines::Counts' unless counts.is_a?(BudgetMachines::Counts)

        super
      end
    end
  end

  # The long and short window readings one rule is judged on. Each is a
  # Sample: :no_data or a Sample::Counts.
  Windows = Data.define(:long, :short) do
    def initialize(long:, short:)
      [long, short].each do |sample|
        next if sample == :no_data || sample.is_a?(Sample::Counts)

        raise TypeError, 'a window reading is :no_data or a BudgetMachines::Sample::Counts'
      end
      super
    end
  end

  # A burn-rate rule: fire at `fire_milli`, clear below `clear_milli`, with
  # `0 < clear_milli <= fire_milli` and `severity` in 0..255.
  Rule = Data.define(:fire_milli, :clear_milli, :severity) do
    def initialize(fire_milli:, clear_milli:, severity:)
      Unsigned.check(fire_milli, 32)
      Unsigned.check(clear_milli, 32)
      Unsigned.check(severity, 8)
      raise RangeError, 'thresholds need 0 < clear_milli <= fire_milli' if clear_milli.zero? || clear_milli > fire_milli

      super
    end
  end

  # One rule's outcome. `state` is :no_data, :quiet or :firing; `change` is
  # :none, :raised or :cleared; `measured` is false when the no-data policy decided.
  # `since` is the instant the rule entered `state`, in Integer nanoseconds as
  # passed to `Monitor#evaluate`, or nil if it never left its start.
  Decision = Data.define(:state, :change, :measured, :long_milli, :short_milli, :since) do
    def firing? = state == :firing
    def no_data? = !measured
  end

  # Every rule's decision for one evaluation, in rule order, and the index of
  # the firing rule with the highest severity (nil when nothing fires).
  Verdict = Data.define(:decisions, :worst)
end
