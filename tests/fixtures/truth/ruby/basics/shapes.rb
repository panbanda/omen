require_relative 'util'

module Describable
  def describe
    "shape #{area}"
  end
end

class Shape
  include Describable

  def initialize(name)
    @name = name
  end

  def area
    0.0
  end

  def label
    @name
  end
end

class Circle < Shape
  def self.unit
    build(1.0)
  end

  def self.build(radius)
    new(radius)
  end
  private_class_method :build

  def initialize(radius)
    super('circle')
    @radius = radius
  end

  def area
    self.scaled(3.14)
  end

  def diameter
    radius * 2
  end

  def label
    super.upcase
  end

  protected

  def radius
    @radius
  end

  private

  def scaled(factor)
    factor * @radius * @radius * Util.twice(1)
  end
end

class Square < Shape
  def initialize(side)
    super('square')
    @side = side
  end

  def area
    @side * @side
  end
end
