package api

import (
	"example.dev/context"
	"example.dev/web"
)

func reqToken(ctx *context.Context) {}

func reqAdmin(ctx *context.Context) {}

func loadUser(ctx *context.Context) {}

func ListUsers(ctx *context.Context) {}

func GetUser(ctx *context.Context) {}

func CreateUser(ctx *context.Context) {}

func Version(ctx *context.Context) {}

func addUserRoutes(m *web.Router) {
	m.Get("", ListUsers)
	m.Combo("/{id}", loadUser).
		Get(GetUser).
		Post(reqAdmin, CreateUser)
}

func Routes() *web.Router {
	m := web.NewRouter()
	m.Group("/v1", func() {
		m.Get("/version", Version)
		m.Group("/users", func() {
			addUserRoutes(m)
		}, reqToken)
		m.Methods("GET, HEAD", "/ping", Version)
	})
	return m
}
