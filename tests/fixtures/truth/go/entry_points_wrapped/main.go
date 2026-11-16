package main

import "net/http"

func list(w http.ResponseWriter, r *http.Request) {}

func show(w http.ResponseWriter, r *http.Request) {}

func auth(next http.Handler) http.Handler { return next }

func audit(next http.Handler) http.Handler { return next }

func authFunc(next http.HandlerFunc) http.HandlerFunc { return next }

func combine(first, second http.HandlerFunc) http.HandlerFunc { return first }

func routes() {
	http.Handle("/p", auth(http.HandlerFunc(show)))
	http.HandleFunc("/q", authFunc(show))
	http.Handle("/r", audit(auth(http.HandlerFunc(show))))
	http.HandleFunc("/s", combine(list, show))
	http.HandleFunc("GET /w/{id}", authFunc(show))
}
