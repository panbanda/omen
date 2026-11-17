class HooksController < ApplicationController
  skip_before_action :require_login

  def receive
  end
end
