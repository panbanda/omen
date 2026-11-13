require 'set'
require 'json'
require_relative 'shapes'
require_relative 'util'
require_relative 'other'

Tools = Util

def local
  4
end

def bare_parens
  local()
end

def bare_no_parens
  local
end

def imported
  only_here()
end

def imported_no_parens
  helper
end

def aliased
  Tools.helper
end

def qualified
  Util.helper + Util.twice(2)
end

def construct
  circle = Circle.new(1.0)
  circle.area
end

def factory
  Circle.unit
end

def outer
  def inner
    5
  end
  inner()
end

def twice_local(value)
  value * 2
end

def callback
  [1, 2].map { |n| twice_local(n) }
end

def closure
  add = ->(n) { n + 1 }
  add.call(2)
end

def shadow
  helper = -> { 9 }
  helper.call
end

def dynamic(shape)
  shape.area
end

def external
  seen = Set.new
  list = []
  list.push(1)
  seen.add(list.length)
  JSON.generate(seen.to_a)
end

def main
  total = bare_parens + bare_no_parens + imported + imported_no_parens + aliased + qualified
  circle = factory
  size = dynamic(Square.new(2.0))
  puts [total, construct, circle.diameter, circle.label, circle.describe, size].join(' ')
  puts [outer, callback.sum, closure, shadow, external()].join(' ')
end

main
