def block_eval(block)
  instance_eval(&block)
  instance_eval { 1 }
end
