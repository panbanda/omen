autoload :Store, "#{__dir__}/store"
autoload :Greeting, 'greeting'

class Bot
  extend Greeting
end

module Loud
  def say
    super.upcase
  end
end

class Speaker
  prepend Loud

  def say
    'hello'
  end
end

Point = Struct.new(:x, :y) do
  def norm
    (x * x) + (y * y)
  end
end

class Flags
  define_method(:ready?) { true }
end

class Report
  def initialize
    @store = Store.open
  end

  def fill
    @store.add(1)
  end
end

def main
  puts Bot.greet
  speaker = Speaker.new
  puts speaker.say
  point = Point.new(3, 4)
  puts point.norm
  flags = Flags.new
  puts flags.ready?
  Report.new.fill
end

main
