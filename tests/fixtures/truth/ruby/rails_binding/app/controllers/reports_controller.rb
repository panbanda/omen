class ReportsController < ApplicationController
  include Trackable

  before_action :check_reports

  def index
  end
end
