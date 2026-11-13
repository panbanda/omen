package shapes

type Circle struct{ radius float64 }

type Square struct{ side float64 }

func (c Circle) Area() float64 {
	return 3.14 * c.radius * c.radius
}

func (s Square) Area() float64 {
	return s.side * s.side
}
