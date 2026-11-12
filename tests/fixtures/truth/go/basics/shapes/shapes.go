package shapes

import "math"

type Shape interface {
	Area() float64
}

type Circle struct {
	Radius float64
}

type Square struct {
	Side float64
}

type Scale = float64

func NewCircle(radius float64) *Circle {
	return &Circle{Radius: radius}
}

func (c *Circle) Area() float64 {
	return c.scaled(math.Pi)
}

func (c *Circle) scaled(factor Scale) float64 {
	return factor * c.Radius * c.Radius
}

func (s Square) Area() float64 {
	return s.Side * s.Side
}

func Describe() string {
	return "shape"
}

func hidden() int {
	return 7
}
