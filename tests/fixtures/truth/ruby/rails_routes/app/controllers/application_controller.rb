class ApplicationController < ActionController::Base
  before_action :require_login
end
