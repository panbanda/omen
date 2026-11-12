package main

import (
	_ "embed"
	"fmt"
	"strings"

	. "example.com/shop/other"
	"example.com/shop/shapes"
	tools "example.com/shop/util"
)

func local() int {
	return 4
}

func run() int {
	total := local() + tools.Helper() + OnlyHere()
	circle := shapes.NewCircle(1)
	_ = circle.Area()
	numbers := apply([]int{1, 2}, func(n int) int { return twiceLocal(n) })
	add := func(n int) int { return n + 1 }
	return total + add(len(numbers))
}

func apply(values []int, f func(int) int) []int {
	out := make([]int, 0, len(values))
	for _, v := range values {
		out = append(out, f(v))
	}
	return out
}

func twiceLocal(value int) int {
	return value * 2
}

func measure(size shapes.Square) float64 {
	return size.Area()
}

func shadow() int {
	Helper := func() int { return 9 }
	return Helper()
}

func dynamic(shape shapes.Shape) float64 {
	return shape.Area()
}

func external() string {
	var b strings.Builder
	b.WriteString("x")
	return strings.ToUpper(b.String())
}

func main() {
	size := measure(shapes.Square{Side: 2})
	fmt.Println(run(), shadow(), shapes.Describe(), size, dynamic(shapes.NewCircle(1)), external())
}
