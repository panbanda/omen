require_relative "session"
require_relative "tools"

class Admin < Vendor::Base
  include Session

  def show
    current_user
  end
end
