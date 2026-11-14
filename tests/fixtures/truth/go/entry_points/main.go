package main

import (
	"net/http"

	"github.com/gin-gonic/gin"
)

func health(w http.ResponseWriter, r *http.Request) {}

func main() {
	http.HandleFunc("/health", health)
	router := gin.Default()
	router.GET("/users/:id", auth, show)
}
