# frozen_string_literal: true

require_relative 'store'
require_relative 'report'

puts Report.new.fill
Board.cache
puts Board.fill
puts Board.new.fill
