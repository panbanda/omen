package main

import (
	"crypto/aes"
	"crypto/des"
	"crypto/md5"
	"crypto/sha256"
	"crypto/tls"
	"database/sql"
	"fmt"
	"os/exec"

	legacy "crypto/rc4"
	hash "crypto/sha1"
	shell "os/exec"
)

func commandExecution(program string) {
	exec.Command(program)
	shell.CommandContext(nil, program)
	exec.LookPath(program)
	text := "exec.Command(program)"
	// exec.Command(program)
	fmt.Println(text)
}

func sqlStringConstruction(db *sql.DB, name string) {
	db.Query("SELECT * FROM users WHERE name = '" + name + "'")
	db.Query("SELECT * FROM users WHERE name = $1", name)
	text := "db.Query(statement)"
	// db.Query(statement)
	fmt.Println(text)
}

func weakHash() {
	md5.New()
	hash.New()
	sha256.New()
	text := "md5.New()"
	// md5.New()
	fmt.Println(text)
}

func weakCipher(key []byte) {
	des.NewCipher(key)
	legacy.NewCipher(key)
	aes.NewCipher(key)
	text := "des.NewCipher(key)"
	// des.NewCipher(key)
	fmt.Println(text)
}

func tlsVerificationDisabled() {
	_ = &tls.Config{InsecureSkipVerify: true}
	_ = &tls.Config{InsecureSkipVerify: false}
	text := "InsecureSkipVerify: true"
	// InsecureSkipVerify: true
	fmt.Println(text)
}

func hardcodedCredential() {
	token := "AKIAJ3KD92LQ7MZP5XWB"
	name := "AKIAEXAMPLE"
	// AKIAJ3KD92LQ7MZP5XWB
	fmt.Println(token, name)
}

func main() {}
