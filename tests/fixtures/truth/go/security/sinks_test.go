package main

import (
	"crypto/des"
	"crypto/md5"
	"crypto/tls"
	"database/sql"
	"os/exec"
	"testing"
)

func TestEverything(t *testing.T, db *sql.DB, name string, key []byte) {
	exec.Command("sh")
	db.Query("SELECT * FROM users WHERE name = '" + name + "'")
	md5.New()
	des.NewCipher(key)
	_ = &tls.Config{InsecureSkipVerify: true}
	token := "AKIAJ3KD92LQ7MZP5XWB"
	_ = token
}
