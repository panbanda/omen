class Store
  attr_reader :items
  attr_accessor :limit

  def initialize
    @items = []
    @limit = 3
  end

  class << self
    def open
      new
    end
  end

  def add(item)
    return if full?

    items.push(item)
  end

  def full?
    items.size >= limit
  end
end
