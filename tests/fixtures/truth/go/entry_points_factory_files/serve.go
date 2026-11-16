package main

import (
	"net/http"

	"github.com/go-chi/chi/v5"
)

func serve() {
	root := chi.NewRouter()
	root.Mount("/admin", AdminRouter())
	http.ListenAndServe(":8080", NewRouter())
}
