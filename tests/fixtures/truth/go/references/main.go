package main

import (
	"net/http"
	"os"
	"os/exec"
	"time"
)

var Cleanup func()

func audit() {
	exec.Command("logger", os.Getenv("EVENT")).Run()
}

func createJob(w http.ResponseWriter, r *http.Request) {
	load := func() {
		exec.Command("sh", "-c", r.URL.Query().Get("x")).Run()
	}
	time.AfterFunc(time.Second, load)
	time.AfterFunc(time.Second, audit)
}

func shadowed(w http.ResponseWriter, r *http.Request) {
	var audit func()
	time.AfterFunc(time.Second, audit)
}

func otherPackage(w http.ResponseWriter, r *http.Request) {
	time.AfterFunc(time.Second, Cleanup)
}

func textOnly(w http.ResponseWriter, r *http.Request) {
	// time.AfterFunc(time.Second, audit)
	w.Write([]byte("audit"))
}

func main() {
	http.HandleFunc("/jobs", createJob)
	http.HandleFunc("/shadow", shadowed)
	http.HandleFunc("/other", otherPackage)
	http.HandleFunc("/text", textOnly)
}
