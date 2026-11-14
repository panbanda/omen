require "json"
require_relative "util"

def helper
  1
end

def run(items)
  puts "x"
  File.join("a", "b")
  helper
  raise ArgumentError, "x"
end

def decoy
  format("%d", 1)
end
