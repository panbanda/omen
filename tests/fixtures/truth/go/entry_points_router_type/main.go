package main

import (
	"net/http"

	"example.dev/web"
	"github.com/go-chi/chi/v5"
)

func Home(w http.ResponseWriter, r *http.Request) {}

func mw(next http.Handler) http.Handler { return next }

func reqSignIn(w http.ResponseWriter, r *http.Request) {}

func main() {
	c := chi.NewRouter()
	c.With(mw).Get("/direct", Home)
	m := web.NewRouter()
	m.Get("/wrapped", reqSignIn, Home)
	http.ListenAndServe(":80", c)
}
