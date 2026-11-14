require_relative "other"

class Tool
  def run
    live_direct
    handler = method(:live_value)
    handler.call
  end

  def live_public; end

  private

  def live_direct; end

  def live_value; end

  def dead_plain; end

  def dead_caller
    dead_callee
  end

  def dead_callee; end

  def dead_unresolved; end
end

Tool.new.run
run_other(nil)
