# frozen_string_literal: true
require 'minitest/autorun'
require 'budget_machines'

class LedgerTest < Minitest::Test
  include BudgetMachines

  MAX = 2**64 - 1

  def counts(good, total) = Counts.new(good, total)

  def test_constants_and_objective_arithmetic
    assert_equal 8, BudgetMachines::CAPACITY
    assert_equal 1000, BudgetMachines::MILLI
    slo = Objective.new(999, 1000)
    assert_equal [999, 1000, 1], [slo.good, slo.per, slo.tolerated]
    assert_equal 14_400, slo.burn_milli(counts(9856, 10_000))
    assert_equal 14_400, slo.burn_milli_of(144, 10_000)
    assert_equal 0, slo.burn_milli(counts(10, 10))
    assert_nil slo.burn_milli(counts(0, 0))
    assert_equal 1000, Objective.new(2, 3).burn_milli(counts(2, 3))
    assert_equal 100, slo.allowance(100_000)
    assert_equal MAX, Objective.new(0, 2**32 - 1).allowance(MAX)
    assert slo.burns_at_least(counts(9856, 10_000), 14_400)
    refute slo.burns_at_least(counts(9857, 10_000), 14_400)
    refute slo.burns_at_least(counts(0, 0), 1)
    assert_raises(RangeError) { Objective.new(1000, 1000) }
    assert_raises(RangeError) { Objective.new(1, 2**32) }
    assert_raises(TypeError) { Objective.new(0.5, 1) }
    assert_raises(TypeError) { slo.allowance(0.5) }
    assert_raises(TypeError) { slo.burn_milli_of(nil, 1) }
    assert_raises(TypeError) { slo.burns_at_least(counts(1, 1), 1.0) }
    assert_predicate slo, :frozen?
    %i[
      __allowance __burn_milli_of __burn_milli __burns_at_least __counts __checked_add __objective
      __rule __u64 __sample __windows __account __decision __verdict
    ].each do |name|
      assert_raises(NoMethodError) { BudgetMachines.public_send(name) }
    end
  end

  def test_counts
    window = counts(995, 1000)
    assert_equal [995, 1000, 5], [window.good, window.total, window.bad]
    assert_equal counts(996, 1002), window.checked_add(counts(1, 2))
    assert_nil counts(0, MAX).checked_add(counts(0, 1))
    assert_raises(RangeError) { counts(2, 1) }
    assert_raises(RangeError) { counts(0, 2**64) }
    assert_raises(RangeError) { counts(-1, 1) }
    [1.0, nil, '1'].each { |value| assert_raises(TypeError) { counts(value, 1) } }
    assert_raises(TypeError) { window.checked_add([1, 1]) }
    assert_raises(TypeError) { Objective.new(99, 100).burn_milli(Sample::Counts.new(window)) }
  end

  def test_accounting_from_no_data_to_exhausted
    slo = Objective.new(99, 100)
    ledger = Ledger.new(slo, 7)
    assert_equal [slo, 7, counts(0, 0)], [ledger.objective, ledger.period, ledger.counts]
    empty = ledger.account
    assert_instance_of Account, empty
    assert_equal :no_data, empty.standing
    assert_equal :no_data, empty.standing
    assert_nil empty.consumed_milli

    assert_equal :ok, ledger.record(7, counts(995, 1000))
    assert_equal counts(995, 1000), ledger.counts
    account = ledger.account
    assert_equal Account.new(
      period: 7, counts: counts(995, 1000), base: 1000, allowance: 10,
      standing: Standing::Within.new(remaining: 5), consumed_milli: 500
    ), account
    assert_equal %i[period counts base allowance standing consumed_milli], Account.members
    assert_kind_of Standing::Within, account.standing
    refute_kind_of Standing::Exhausted, account.standing
    assert_predicate account, :frozen?

    assert_equal :ok, ledger.record(7, counts(0, 6))
    spent = ledger.account
    assert_equal Standing::Exhausted.new(overspent: 1), spent.standing
    assert_kind_of Standing::Exhausted, spent.standing

    assert_equal :ok, ledger.replace(7, counts(1000, 1000))
    assert_equal Standing::Within.new(remaining: 10), ledger.account.standing
  end

  def test_account_against_a_planned_total
    ledger = Ledger.new(Objective.new(999, 1000), 0)
    ledger.record(0, counts(9, 10))
    assert_kind_of Standing::Exhausted, ledger.account.standing
    planned = ledger.account_against(100_000)
    assert_equal [100_000, 100, Standing::Within.new(99), 10],
                 [planned.base, planned.allowance, planned.standing, planned.consumed_milli]
    assert_equal :below_observed, ledger.account_against(9)
    unseen = Ledger.new(Objective.new(999, 1000), 0).account_against(1000)
    assert_equal [:no_data, 1, 0], [unseen.standing, unseen.allowance, unseen.consumed_milli]
  end

  def test_refusals_change_nothing
    ledger = Ledger.new(Objective.new(9, 10), 5)
    assert_equal :ok, ledger.record(5, counts(MAX - 1, MAX - 1))
    before = ledger.account
    assert_equal :stale_period, ledger.record(4, counts(1, 1))
    assert_equal :stale_period, ledger.record(6, counts(1, 1))
    assert_equal :stale_period, ledger.replace(4, counts(1, 1))
    assert_equal :overflow, ledger.record(5, counts(2, 2))
    assert_equal :stale_period, ledger.roll(5)
    assert_equal :stale_period, ledger.roll(4)
    assert_equal before, ledger.account
  end

  def test_roll_closes_the_period_and_resets
    ledger = Ledger.new(Objective.new(9, 10), 5)
    ledger.record(5, counts(8, 10))
    closed = ledger.roll(MAX)
    assert_equal [5, counts(8, 10), Standing::Exhausted.new(1)], [closed.period, closed.counts, closed.standing]
    assert_equal [MAX, counts(0, 0)], [ledger.period, ledger.counts]
    assert_equal :no_data, ledger.account.standing
    assert_equal :stale_period, ledger.record(5, counts(1, 1))
  end

  def test_strict_inputs_before_mutation
    slo = Objective.new(9, 10)
    ledger = Ledger.new(slo, 0)
    [1.0, Float::NAN, nil, '1', Object.new, [9, 10]].each do |value|
      assert_raises(TypeError) { Ledger.new(value, 0) }
      assert_raises(TypeError) { Ledger.new(slo, value) }
      assert_raises(TypeError) { ledger.record(value, counts(1, 1)) }
      assert_raises(TypeError) { ledger.record(0, value) }
      assert_raises(TypeError) { ledger.replace(0, value) }
      assert_raises(TypeError) { ledger.roll(value) }
      assert_raises(TypeError) { ledger.account_against(value) }
    end
    [-1, 2**64].each { |n| assert_raises(RangeError) { ledger.roll(n) } }
    assert_raises(ArgumentError) { Ledger.new(slo) }
    assert_raises(TypeError) { Ledger.allocate }
    assert_equal counts(0, 0), ledger.counts
  end

  def test_freeze_guards
    ledger = Ledger.new(Objective.new(9, 10), 0)
    ledger.record(0, counts(1, 1))
    ledger.freeze
    assert_raises(FrozenError) { ledger.record(0, counts(1, 1)) }
    assert_raises(FrozenError) { ledger.replace(0, counts(1, 1)) }
    assert_raises(FrozenError) { ledger.roll(1) }
    # Frozen is checked first, before the arguments are read.
    assert_raises(FrozenError) { ledger.record(-1, :counts) }
    assert_raises(FrozenError) { ledger.replace(1.5, nil) }
    assert_raises(FrozenError) { ledger.roll('1') }
    assert_kind_of Standing::Within, ledger.account.standing
    assert_kind_of Standing::Within, ledger.account_against(10).standing
  end
end
