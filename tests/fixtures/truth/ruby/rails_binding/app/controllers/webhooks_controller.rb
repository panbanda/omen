class WebhooksController < ApplicationController
  skip_before_action :require_login
  before_action :check_signature

  def aws
  end
end
