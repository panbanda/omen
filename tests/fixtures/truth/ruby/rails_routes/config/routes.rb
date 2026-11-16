Rails.application.routes.draw do
  root "pages#home"
  resources :users, only: [:show, :update]
  resource :profile, only: [:show]
  namespace :admin do
    resources :reports, only: [:index] do
      member do
        get :download
      end
    end
  end
  scope "/api", module: "v1" do
    get "/status", to: "status#show"
  end
  match "/hook", to: "hooks#receive", via: [:get, :post]
  get "/legacy", controller: "pages", action: "legacy"
  get "/skipped/#{name}", to: "pages#home"
  %w[a b].each do |part|
    get "/#{part}", to: "pages#home"
  end
  mount Sidekiq::Web => "/sidekiq"
end
