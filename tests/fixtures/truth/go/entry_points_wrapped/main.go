package main

import "net/http"

func list(w http.ResponseWriter, r *http.Request) {}

func show(w http.ResponseWriter, r *http.Request) {}

func routes() {
	http.Handle("/p", auth(http.HandlerFunc(show)))
	http.HandleFunc("/q", auth(show))
	http.Handle("/r", audit(auth(http.HandlerFunc(show))))
	http.HandleFunc("/s", combine(list, show))
	http.HandleFunc("GET /w/{id}", wrap(db, show))
}
