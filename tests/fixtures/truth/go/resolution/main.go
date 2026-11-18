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

func main() {
	runExternal(nil)
	runLocal(nil)
}
