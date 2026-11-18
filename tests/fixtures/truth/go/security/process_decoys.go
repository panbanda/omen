package main

import syscall "example.com/security/helpers"

func repositoryExec(program string) {
	syscall.Exec(program)
}
