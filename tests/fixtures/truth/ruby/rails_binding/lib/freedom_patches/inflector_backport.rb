module ActiveSupport
  module Inflector
    CACHES = []

    def self.memoize(*args)
      args.each do |method_name|
        cache = {}
        CACHES << cache

        uncached = "#{method_name}_without_cache"
        alias_method uncached, method_name

        define_method(method_name) do |*arguments|
          cache[arguments] ||= public_send(uncached, *arguments)
        end
      end
    end

    memoize :pluralize, :singularize, :camelize, :underscore, :classify
  end
end
