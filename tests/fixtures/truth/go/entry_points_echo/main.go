package main

import (
	"github.com/labstack/echo/v4"
	"github.com/labstack/echo/v4/middleware"
)

func list(c echo.Context) error { return nil }

func show(c echo.Context) error { return nil }

func routes() *echo.Echo {
	e := echo.New()
	e.GET("/health", list)
	e.Use(middleware.Logger())
	api := e.Group("/api", auth)
	api.GET("/users/:id", show, audit)
	admin := api.Group("/admin", requireRoot)
	admin.Use(extra)
	admin.POST("/users", list)
	api.PUT("/late", list)
	e.Match([]string{"GET", "POST"}, "/multi", list)
	return e
}
