package main

import (
	"fmt"
	str "strings"
)

type Sink struct{}

func (s Sink) Write(text string) {}

func run(items []int) {
	fmt.Println(len(items))
	items = append(items, 1)
	var builder str.Builder
	builder.WriteString("x")
	var sink Sink
	sink.Write("x")
	copy(items, items)
}
