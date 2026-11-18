Rails.application.routes.draw do
  post '/jobs', to: 'jobs#create'
  post '/shadow', to: 'jobs#shadowed'
  post '/other', to: 'jobs#other_file'
  post '/text', to: 'jobs#text_only'
end
