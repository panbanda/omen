class Other
  def cleanup
    system("rm " + ENV["TARGET"])
  end
end
