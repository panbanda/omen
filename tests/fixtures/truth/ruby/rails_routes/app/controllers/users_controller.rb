class UsersController < ApplicationController
  before_action :authorize_owner, only: [:update]
  skip_before_action :require_login, only: [:show]

  def show
  end

  def update
  end
end
