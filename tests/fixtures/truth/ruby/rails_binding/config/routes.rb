Rails.application.routes.draw do
  def patch(*)
  end

  scope path: nil, constraints: { format: %w[json html] } do
    post "webhooks/aws" => "webhooks#aws"
    get "old" => redirect("/new")
    namespace :admin, constraints: StaffConstraint.new do
      get "users" => "users#index"
      resources :users, only: %i[show update]
      delete "users/:id" => "users#destroy", :constraints => AdminConstraint.new
    end
    %w[users u].each do |root_path|
      get "#{root_path}/:username" => "users#show", :format => :json
    end
    FILTERS.each do |filter|
      get "#{filter}" => "list##{filter}"
    end
    get "session/current"
    patch "session/current" => "session#update"
  end
end
