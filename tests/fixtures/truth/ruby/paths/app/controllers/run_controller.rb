class RunController < ApplicationController
  before_action :authenticate_user!

  def show
    prepare(params[:x])
  end

  private

  def prepare(value)
    execute(value)
  end

  def execute(value)
    system(value)
  end

  def orphan(value)
    system(value)
  end
end
