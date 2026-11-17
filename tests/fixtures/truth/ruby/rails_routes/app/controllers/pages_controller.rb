class PagesController < ApplicationController
  skip_before_action :require_login, only: [:home]

  def home
  end

  def legacy
  end
end
