package main

import (
	"net/http"

	"example.dev/api"
	"example.dev/web"
)

func Home(w http.ResponseWriter, r *http.Request) {}

func NormalRoutes() *web.Router {
	r := web.NewRouter()
	r.Mount("/api", api.Routes())
	r.Any("/", Home)
	return r
}

func main() {
	http.ListenAndServe(":80", NormalRoutes())
}
