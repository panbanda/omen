# frozen_string_literal: true

require_relative 'problem'
require_relative 'blog'
require_relative 'locations'

class Check
  def problem
    found = Problem.new('a')
    found.text
  end
end

module Api
  class Contract
    def build
      Body::Location.for('b')
    end
  end
end

puts Check.new.problem
puts Api::Contract.new.build
