class JobsController < ApplicationController
  def create
    load_module = lambda do
      eval(params[:code])
    end
    schedule(load_module)
    schedule(method(:audit))
    head :ok
  end

  def shadowed
    audit = params[:audit]
    schedule(audit)
    head :ok
  end

  def other_file
    schedule(method(:cleanup))
    head :ok
  end

  def text_only
    # schedule(method(:audit))
    schedule("audit")
    schedule(:audit)
    head :ok
  end

  private

  def audit
    system("logger " + ENV["EVENT"])
  end

  def schedule(task)
    task
  end
end
