require "digest"
require "openssl"
require "yaml"

module Sinks
  def self.command_execution(command)
    system(command)
    Process.pid
    text = "system(command)"
    # system(command)
    text
  end

  def self.code_evaluation(expression)
    eval(expression)
    Integer(expression)
    text = "eval(expression)"
    # eval(expression)
    text
  end

  def self.unsafe_deserialization(blob, text)
    Marshal.load(blob)
    YAML.safe_load(text)
    text = "Marshal.load(blob)"
    # Marshal.load(blob)
    text
  end

  def self.sql_string_construction(connection, name)
    connection.execute("SELECT * FROM users WHERE name = '#{name}'")
    connection.execute("SELECT 1")
    text = "connection.execute(query)"
    # connection.execute(query)
    text
  end

  def self.weak_hash(data)
    Digest::MD5.hexdigest(data)
    Digest::SHA256.hexdigest(data)
    text = "Digest::MD5.hexdigest(data)"
    # Digest::MD5.hexdigest(data)
    text
  end

  def self.weak_cipher
    OpenSSL::Cipher.new("des-ecb")
    OpenSSL::Cipher.new("aes-256-gcm")
    text = "OpenSSL::Cipher.new('des-ecb')"
    # OpenSSL::Cipher.new("des-ecb")
    text
  end

  def self.tls_verification_disabled(http)
    http.verify_mode = OpenSSL::SSL::VERIFY_NONE
    http.verify_mode = OpenSSL::SSL::VERIFY_PEER
    text = "OpenSSL::SSL::VERIFY_NONE"
    # OpenSSL::SSL::VERIFY_NONE
    text
  end

  def self.hardcoded_credential
    token = "AKIAIOSFODNN7EXAMPLE"
    name = "AKIAEXAMPLE"
    # AKIAIOSFODNN7EXAMPLE
    [token, name]
  end
end
