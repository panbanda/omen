class ExportsController < ApplicationController
  before_action :check_export, only: [:show], if: :limited?

  def index
  end

  def show
  end
end
