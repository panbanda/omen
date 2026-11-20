# frozen_string_literal: true

module Blog
  class Problem
    def initialize(text)
      @text = text
    end

    def text
      "blog #{@text}"
    end
  end
end
