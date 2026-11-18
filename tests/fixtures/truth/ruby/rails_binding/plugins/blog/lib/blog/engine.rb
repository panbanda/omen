module Blog
  class Engine < ::Rails::Engine
    engine_name "blog"
    isolate_namespace Blog
  end
end
