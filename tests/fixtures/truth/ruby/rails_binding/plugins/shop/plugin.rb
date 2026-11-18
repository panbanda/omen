Rails.application.routes.append do
  mount Shop::Engine, at: "/shop"
  mount Shop::Engine, at: "/store"
end
