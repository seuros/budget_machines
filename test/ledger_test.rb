# frozen_string_literal: true
require 'minitest/autorun'
require 'budget_machines'

class LedgerTest < Minitest::Test
  MAX = 2**64 - 1

  def test_constants_and_module_arithmetic
    assert_equal 8, BudgetMachines::CAPACITY
    assert_equal 1000, BudgetMachines::MILLI
    assert_equal 14_400, BudgetMachines.burn_milli(999, 1000, 9856, 10_000)
    assert_equal 0, BudgetMachines.burn_milli(999, 1000, 10, 10)
    assert_nil BudgetMachines.burn_milli(999, 1000, 0, 0)
    assert_equal 1000, BudgetMachines.burn_milli(2, 3, 2, 3)
    assert_equal 100, BudgetMachines.allowance(999, 1000, 100_000)
    assert_equal MAX, BudgetMachines.allowance(0, 2**32 - 1, MAX)
    assert_raises(RangeError) { BudgetMachines.burn_milli(1000, 1000, 1, 1) }
    assert_raises(RangeError) { BudgetMachines.burn_milli(999, 1000, 2, 1) }
    assert_raises(RangeError) { BudgetMachines.allowance(1, 1, 10) }
    assert_raises(TypeError) { BudgetMachines.allowance(0.5, 1, 10) }
  end

  def test_accounting_from_no_data_to_exhausted
    ledger = BudgetMachines::Ledger.new(99, 100, 7)
    empty = ledger.account
    assert_instance_of BudgetMachines::Account, empty
    assert_equal :no_data, empty.standing
    assert_predicate empty, :no_data?
    assert_nil empty.consumed_milli

    assert_equal :ok, ledger.record(7, 995, 1000)
    assert_equal [7, 995, 1000], [ledger.period, ledger.good, ledger.total]
    account = ledger.account
    assert_equal BudgetMachines::Account.new(
      period: 7, good: 995, total: 1000, base: 1000, allowance: 10,
      standing: :within, remaining: 5, overspent: 0, consumed_milli: 500
    ), account
    assert_equal 5, account.bad
    refute_predicate account, :exhausted?
    assert_predicate account, :frozen?

    assert_equal :ok, ledger.record(7, 0, 6)
    spent = ledger.account
    assert_equal [:exhausted, 0, 1], [spent.standing, spent.remaining, spent.overspent]
    assert_predicate spent, :exhausted?

    assert_equal :ok, ledger.replace(7, 1000, 1000)
    assert_equal [:within, 10], [ledger.account.standing, ledger.account.remaining]
  end

  def test_account_against_a_planned_total
    ledger = BudgetMachines::Ledger.new(999, 1000, 0)
    ledger.record(0, 9, 10)
    assert_equal :exhausted, ledger.account.standing
    planned = ledger.account_against(100_000)
    assert_equal [100_000, 100, :within, 99, 10],
                 [planned.base, planned.allowance, planned.standing, planned.remaining, planned.consumed_milli]
    assert_equal :below_observed, ledger.account_against(9)
    unseen = BudgetMachines::Ledger.new(999, 1000, 0).account_against(1000)
    assert_equal [:no_data, 1, 0], [unseen.standing, unseen.remaining, unseen.consumed_milli]
  end

  def test_refusals_change_nothing
    ledger = BudgetMachines::Ledger.new(9, 10, 5)
    assert_equal :ok, ledger.record(5, MAX - 1, MAX - 1)
    before = ledger.account
    assert_equal :stale_period, ledger.record(4, 1, 1)
    assert_equal :stale_period, ledger.record(6, 1, 1)
    assert_equal :stale_period, ledger.replace(4, 1, 1)
    assert_equal :overflow, ledger.record(5, 2, 2)
    assert_equal :stale_period, ledger.roll(5)
    assert_equal :stale_period, ledger.roll(4)
    assert_equal before, ledger.account
  end

  def test_roll_closes_the_period_and_resets
    ledger = BudgetMachines::Ledger.new(9, 10, 5)
    ledger.record(5, 8, 10)
    closed = ledger.roll(MAX)
    assert_equal [5, 8, 10, :exhausted], [closed.period, closed.good, closed.total, closed.standing]
    assert_equal [MAX, 0, 0], [ledger.period, ledger.good, ledger.total]
    assert_equal :no_data, ledger.account.standing
    assert_equal :stale_period, ledger.record(5, 1, 1)
  end

  def test_strict_inputs_before_mutation
    ledger = BudgetMachines::Ledger.new(9, 10, 0)
    bad = [1.0, Float::NAN, nil, '1', Object.new]
    bad.each do |value|
      assert_raises(TypeError) { BudgetMachines::Ledger.new(value, 10, 0) }
      assert_raises(TypeError) { BudgetMachines::Ledger.new(9, 10, value) }
      assert_raises(TypeError) { ledger.record(value, 1, 1) }
      assert_raises(TypeError) { ledger.record(0, value, 1) }
      assert_raises(TypeError) { ledger.record(0, 1, value) }
      assert_raises(TypeError) { ledger.replace(0, value, 1) }
      assert_raises(TypeError) { ledger.roll(value) }
      assert_raises(TypeError) { ledger.account_against(value) }
    end
    [-1, 2**64].each { |n| assert_raises(RangeError) { ledger.record(0, 0, n) } }
    assert_raises(RangeError) { ledger.record(0, 2, 1) }
    assert_raises(RangeError) { ledger.replace(0, 2, 1) }
    assert_raises(RangeError) { BudgetMachines::Ledger.new(10, 10, 0) }
    assert_raises(RangeError) { BudgetMachines::Ledger.new(1, 2**32, 0) }
    assert_raises(TypeError) { BudgetMachines::Ledger.allocate }
    assert_equal [0, 0], [ledger.good, ledger.total]
  end

  def test_freeze_guards
    ledger = BudgetMachines::Ledger.new(9, 10, 0)
    ledger.record(0, 1, 1)
    ledger.freeze
    assert_raises(FrozenError) { ledger.record(0, 1, 1) }
    assert_raises(FrozenError) { ledger.replace(0, 1, 1) }
    assert_raises(FrozenError) { ledger.roll(1) }
    assert_equal :within, ledger.account.standing
    assert_equal :within, ledger.account_against(10).standing
  end
end
