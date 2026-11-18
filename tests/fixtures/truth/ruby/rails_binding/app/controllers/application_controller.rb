class ApplicationController < ActionController::Base
  before_action :require_login

  def self.requires_login(arg = {})
    @requires_login_arg = arg
  end

  def self.requires_plugin(plugin_name)
    before_action do
      raise PluginDisabled if plugin_disabled?(plugin_name)
    end
  end
end
