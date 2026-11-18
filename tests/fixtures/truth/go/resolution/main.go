package main

import (
	"math/rand/v2"

	"example.com/resolution/local"
	"example.com/resolution/store"
)

func runExternal(source *rand.Rand) int {
	return source.Int()
}

func runLocal(source *local.Rand) int {
	return source.Int()
}

type Context struct {
	*local.Base
	name string
}

func render(ctx *Context) string {
	return ctx.JSON() + ctx.name
}

func main() {
	runExternal(nil)
	runLocal(nil)
	render(&Context{})
	store.Open()
	localGeneric()
}

func first[T any](values []T) T {
	return values[0]
}

func localGeneric() int {
	_ = store.Must[string]("x")
	return first[int]([]int{1})
}
