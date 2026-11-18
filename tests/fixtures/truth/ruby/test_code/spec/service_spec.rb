require_relative "../lib/service"

def shared_service
  Service.new
end

describe Service do
  it "gives one" do
    expect(shared_service.production).to eq(1)
  end
end
