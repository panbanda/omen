# frozen_string_literal: true

class Check
  class Problem
    def initialize(text)
      @text = text
    end

    def text
      "check #{@text}"
    end
  end
end
