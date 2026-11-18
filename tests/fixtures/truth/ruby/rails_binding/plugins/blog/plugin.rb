after_initialize do
  Rails.application.routes.append { mount ::Blog::Engine, at: "/blog", constraints: BlogConstraint.new }
end
