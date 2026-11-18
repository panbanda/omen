package main

import (
	"math/rand/v2"

	"example.com/resolution/local"
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
}
