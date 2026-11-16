module Admin
  class ReportsController < ApplicationController
    before_action :require_admin, except: [:download]

    def index
    end

    def download
    end
  end
end
