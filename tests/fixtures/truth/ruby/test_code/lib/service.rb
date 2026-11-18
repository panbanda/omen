class Service
  def production
    1
  end

  def test_connection
    production
  end
end

context "of a different library" do
  def not_a_spec
    1
  end
end
