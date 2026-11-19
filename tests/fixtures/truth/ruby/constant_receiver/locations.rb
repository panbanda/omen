# frozen_string_literal: true

module Api
  module Body
    class Location
      def self.for(name)
        "api #{name}"
      end
    end
  end
end

module Other
  module Body
    class Location
      def self.for(name)
        "other #{name}"
      end
    end
  end
end
