class Gateway
  def execute(statement)
    statement
  end
end

module Digest
  class SHA1
    def self.hexdigest(data)
      data
    end
  end
end

module Marshal
  def self.restore(blob)
    blob
  end
end

module OpenSSL
  module Cipher
    class RC4
      def self.new(name)
        name
      end
    end
  end

  module SSL
    VERIFY_NONE = 0
  end
end

module Decoys
  def self.exec(command)
    command
  end

  def self.class_eval(source)
    source
  end

  def self.repository_symbols(command, source, blob, name, data)
    exec(command)
    class_eval(source)
    Marshal.restore(blob)
    gateway = Gateway.new
    gateway.execute("SELECT * FROM users WHERE name = '#{name}'")
    Digest::SHA1.hexdigest(data)
    OpenSSL::Cipher::RC4.new(name)
    name.verify_mode = OpenSSL::SSL::VERIFY_NONE
  end
end
