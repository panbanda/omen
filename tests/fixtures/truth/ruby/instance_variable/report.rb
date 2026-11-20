# frozen_string_literal: true

class Report
  def initialize
    @store = Store.new
  end

  def fill
    @store.add(1)
  end
end

class Board
  def self.cache
    @store = Cache.new
  end

  def self.fill
    @store.add(2)
  end

  def initialize
    @store = Store.new
  end

  def fill
    @store.add(3)
  end
end
