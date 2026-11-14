package main

import "os/exec"

func shadowedLater(program string) {
	exec.Command(program)
	{
		exec := struct{}{}
		_ = exec
	}
}
