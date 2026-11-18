class PluginsController < ApplicationController
  requires_login
  requires_plugin "blog"

  def index
  end
end
