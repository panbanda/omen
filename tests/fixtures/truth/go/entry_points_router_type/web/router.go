package web

import (
	"net/http"

	"github.com/go-chi/chi/v5"
)

type Router struct{ chiRouter chi.Router }

func NewRouter() *Router {
	return &Router{chiRouter: chi.NewRouter()}
}

func (r *Router) Get(pattern string, h ...any) {
	r.chiRouter.Get(pattern, h[0].(http.HandlerFunc))
}
