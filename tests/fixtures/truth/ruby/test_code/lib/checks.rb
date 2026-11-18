require "minitest/autorun"
require_relative "service"

class ServiceTest < Minitest::Test
  def setup
    @service = Service.new
  end

  def test_production
    assert_equal 1, value
  end

  def value
    @service.production
  end
end

RSpec.describe Service do
  def build
    Service.new
  end

  context "when new" do
    it "gives one" do
      result = build.production
      expect(result).to eq(1)
    end
  end
end
