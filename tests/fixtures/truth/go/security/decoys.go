package main

import (
	des "example.com/security/helpers"
	exec "example.com/security/helpers"
	md5 "example.com/security/helpers"
	"example.com/security/helpers"
)

func repositorySymbols(program string, key []byte, name string) {
	exec.Command(program)
	md5.New()
	des.NewCipher(key)
	gateway := &helpers.Gateway{}
	gateway.Query("SELECT * FROM users WHERE name = '" + name + "'")
}
