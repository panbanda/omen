Blog::Engine.routes.draw do
  get "/posts" => "posts#index"
  resources :posts, only: :update
end
