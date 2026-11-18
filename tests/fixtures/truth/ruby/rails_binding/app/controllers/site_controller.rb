class SiteController < ApplicationController
  skip_before_action :require_login, only: [:settings]

  def settings
  end
end
