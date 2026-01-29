class OrderProcessor
  def initialize(inventory)
    @inventory = inventory
  end

  def process(order)
    unless order[:items]&.any?
      raise ArgumentError, "order must have items"
    end

    total = 0
    order[:items].each do |item|
      price = @inventory.price_for(item[:sku])
      total += price * item[:quantity]
    end
    total
  end

  private

  def apply_tax(amount, rate)
    amount * (1 + rate)
  end
end
