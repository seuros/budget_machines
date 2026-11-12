# frozen_string_literal: true

require_relative 'budget_machines/version'

module BudgetMachines
  # One compliance period's budget. `standing` is :no_data, :within or :exhausted.
  Account = Data.define(
    :period, :good, :total, :base, :allowance, :standing, :remaining, :overspent, :consumed_milli
  ) do
    def bad = total - good
    def no_data? = standing == :no_data
    def exhausted? = standing == :exhausted
  end

  # One rule's outcome. `state` is :no_data, :quiet or :firing; `change` is
  # nil, :raised or :cleared; `measured` is false when the no-data policy decided.
  Decision = Data.define(:state, :change, :measured, :long_milli, :short_milli, :since) do
    def firing? = state == :firing
    def no_data? = !measured
  end
end

# Native is required: no algorithm fallback.
require 'budget_machines_native/budget_machines_native'
