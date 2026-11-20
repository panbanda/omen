# frozen_string_literal: true

class Store
  def add(item)
    "store #{item}"
  end
end

class Cache
  def add(item)
    "cache #{item}"
  end
end
