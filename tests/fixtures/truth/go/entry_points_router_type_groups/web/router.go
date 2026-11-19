package web

import (
	"net/http"
	"strings"

	"github.com/go-chi/chi/v5"
)

type Router struct {
	chiRouter      *chi.Mux
	curGroupPrefix string
	curMiddlewares []any
}

func NewRouter() *Router {
	r := chi.NewRouter()
	return &Router{chiRouter: r}
}

func (r *Router) Group(pattern string, fn func(), middlewares ...any) {
	previousGroupPrefix := r.curGroupPrefix
	previousMiddlewares := r.curMiddlewares
	r.curGroupPrefix += pattern
	r.curMiddlewares = append(r.curMiddlewares, middlewares...)
	fn()
	r.curGroupPrefix = previousGroupPrefix
	r.curMiddlewares = previousMiddlewares
}

func (r *Router) getPattern(pattern string) string {
	newPattern := r.curGroupPrefix + pattern
	if !strings.HasPrefix(newPattern, "/") {
		newPattern = "/" + newPattern
	}
	return newPattern
}

func toHandler(h []any) http.HandlerFunc { return nil }

func (r *Router) Methods(methods, pattern string, h ...any) {
	fullPattern := r.getPattern(pattern)
	if strings.Contains(methods, ",") {
		for method := range strings.SplitSeq(methods, ",") {
			r.chiRouter.With().Method(strings.TrimSpace(method), fullPattern, toHandler(h))
		}
	} else {
		r.chiRouter.With().Method(methods, fullPattern, toHandler(h))
	}
}

func (r *Router) Mount(pattern string, subRouter *Router) {
	r.chiRouter.Mount(r.getPattern(pattern), subRouter.chiRouter)
}

func (r *Router) Any(pattern string, h ...any) {
	r.chiRouter.HandleFunc(r.getPattern(pattern), toHandler(h))
}

func (r *Router) Get(pattern string, h ...any) { r.Methods("GET", pattern, h...) }

func (r *Router) Post(pattern string, h ...any) { r.Methods("POST", pattern, h...) }

func (r *Router) Combo(pattern string, h ...any) *Combo {
	return &Combo{r, pattern, h}
}

type Combo struct {
	r       *Router
	pattern string
	h       []any
}

func (c *Combo) Get(h ...any) *Combo {
	c.r.Get(c.pattern, append(c.h, h...)...)
	return c
}

func (c *Combo) Post(h ...any) *Combo {
	c.r.Post(c.pattern, append(c.h, h...)...)
	return c
}
