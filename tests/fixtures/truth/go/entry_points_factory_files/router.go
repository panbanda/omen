package main

import (
	"net/http"

	"github.com/go-chi/chi/v5"
)

func list(w http.ResponseWriter, r *http.Request) {}

func NewRouter() chi.Router {
	r := chi.NewRouter()
	r.Get("/health", list)
	return r
}

func AdminRouter() chi.Router {
	r := chi.NewRouter()
	r.Get("/users", list)
	return r
}
