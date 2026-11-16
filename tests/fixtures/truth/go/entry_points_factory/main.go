package main

import (
	"net/http"

	"github.com/go-chi/chi/v5"
)

func list(w http.ResponseWriter, r *http.Request) {}

func show(w http.ResponseWriter, r *http.Request) {}

func rootRouter() chi.Router {
	r := chi.NewRouter()
	r.Get("/root", list)
	return r
}

func subRouter() chi.Router {
	s := chi.NewRouter()
	s.Get("/", list)
	return s
}

func serve() {
	r := rootRouter()
	r.Get("/extra", show)
	r.Mount("/sub", subRouter())
	http.ListenAndServe(":8080", r)
}
