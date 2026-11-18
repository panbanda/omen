package main

import (
	"os"
	"syscall"

	kernel "syscall"
)

func startProcess(program string, arguments []string) {
	os.StartProcess(program, arguments, nil)
	kernel.ForkExec(program, arguments, nil)
	syscall.Getpid()
	text := "syscall.Exec(program)"
	// os.StartProcess(program, arguments, nil)
	_ = text
}
