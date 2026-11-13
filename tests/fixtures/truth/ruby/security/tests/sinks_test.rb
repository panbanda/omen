require "digest"
require "openssl"

module SinksTest
  def self.test_everything(command, blob, connection, name, http)
    system(command)
    eval(command)
    Marshal.load(blob)
    connection.execute("SELECT * FROM users WHERE name = '#{name}'")
    Digest::MD5.hexdigest(blob)
    OpenSSL::Cipher.new("des-ecb")
    http.verify_mode = OpenSSL::SSL::VERIFY_NONE
    token = "AKIAIOSFODNN7EXAMPLE"
    token
  end
end
