package main

import (
	"net/http"
	"os/exec"
)

func run(w http.ResponseWriter, r *http.Request) {
	prepare(r.URL.Query().Get("x"))
}

func prepare(value string) {
	execute(value)
}

func execute(value string) {
	exec.Command("sh", "-c", value).Run()
}

func orphan(value string) {
	exec.Command(value).Run()
}

func main() {
	http.HandleFunc("/run", run)
}
