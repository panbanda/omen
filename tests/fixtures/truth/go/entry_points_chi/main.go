package main

import (
	"net/http"

	"github.com/go-chi/chi/v5"
)

func list(w http.ResponseWriter, r *http.Request) {}

func show(w http.ResponseWriter, r *http.Request) {}

func routes() http.Handler {
	r := chi.NewRouter()
	r.Get("/early", list)
	r.Use(logger)
	r.Get("/health", list)
	r.Route("/api", func(r chi.Router) {
		r.Use(auth)
		r.Get("/users/{id}", show)
		r.Group(func(r chi.Router) {
			r.Use(admin)
			r.Post("/users", list)
		})
	})
	r.With(audit).Delete("/items/{id:[0-9]+}", show)
	r.Method("PATCH", "/m", list)
	r.Mount("/sub", subRouter())
	return r
}

func subRouter() chi.Router {
	s := chi.NewRouter()
	s.Get("/", list)
	return s
}
