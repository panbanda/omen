package other

import (
	"os"
	"os/exec"
)

func Cleanup() {
	exec.Command("rm", os.Getenv("TARGET")).Run()
}
