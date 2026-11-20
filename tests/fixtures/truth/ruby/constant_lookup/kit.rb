module Other
  class Fields
    def kind
      "other"
    end
  end
end

module Kit
  module Fields
    def kind
      "kit"
    end
  end

  class Base
    def setup
      "kit base"
    end
  end

  class Form < Base
    include Fields

    class Base
      def setup
        "form base"
      end
    end

    def build
      setup
    end

    def show
      kind
    end
  end
end
