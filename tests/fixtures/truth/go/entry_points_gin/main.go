package main

import (
	"github.com/gin-gonic/gin"
)

func list(c *gin.Context) {}

func show(c *gin.Context) {}

func routes() *gin.Engine {
	r := gin.New()
	r.GET("/early", list)
	r.Use(logger)
	v1 := r.Group("/v1", auth)
	v1.GET("/users/:id", audit, show)
	inner := v1.Group("/inner")
	inner.Use(extra)
	inner.POST("/x", list)
	r.Group("/chain").DELETE("/c", list)
	v1.PUT("/late", list)
	r.GET("/after", list)
	return r
}
