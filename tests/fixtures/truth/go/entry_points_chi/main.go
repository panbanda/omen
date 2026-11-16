package main

import (
	"net/http"

	"github.com/go-chi/chi/v5"
)

func list(w http.ResponseWriter, r *http.Request) {}

func show(w http.ResponseWriter, r *http.Request) {}

func routes() http.Handler {
	r := chi.NewRouter()
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
	sub := chi.NewRouter()
	sub.Get("/", list)
	r.Mount("/sub", sub)
	return r
}
