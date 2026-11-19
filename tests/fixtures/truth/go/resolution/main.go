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
	_ = persist(&local.Disk{})
	_ = convert(nil, 1)
	_ = describe(local.Missing{})
	_ = reuse(&Context{})
}

func first[T any](values []T) T {
	return values[0]
}

func localGeneric() int {
	_ = store.Must[string]("x")
	return first[int]([]int{1})
}

func persist(store local.Store) error {
	return store.Save("a")
}

func convert(values []int, value int) string {
	values = append(values, value)
	return string(rune(len(values)))
}

func describe(failure error) string {
	return failure.Error()
}

func reuse(ctx *Context) string {
	base := ctx.Base // the embedded field
	return base.JSON()
}
