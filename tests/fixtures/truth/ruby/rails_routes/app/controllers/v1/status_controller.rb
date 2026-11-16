module V1
  class StatusController < ApplicationController
    skip_before_action :require_login

    def show
    end
  end
end
