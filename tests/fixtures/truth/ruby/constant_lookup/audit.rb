module Audit
  def audit
    "top"
  end
end

module Deep
  module Audit
    def audit
      "deep"
    end
  end
end

class Deep::Job
  include Audit

  def run
    audit
  end
end
