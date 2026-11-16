package main

import (
	"net/http"

	"github.com/gorilla/mux"
)

func list(w http.ResponseWriter, r *http.Request) {}

func show(w http.ResponseWriter, r *http.Request) {}

func routes() http.Handler {
	r := mux.NewRouter()
	r.HandleFunc("/health", list)
	r.HandleFunc("/users/{id}", show).Methods("GET", "POST")
	r.HandleFunc("/items/{id:[0-9]+}", show).Methods("DELETE").Name("item")
	r.Handle("/handle", http.HandlerFunc(list))
	api := r.PathPrefix("/api").Subrouter()
	api.Use(auth)
	api.HandleFunc("/ping", list).Methods("GET")
	r.Use(logger)
	return r
}
