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

type Group struct {
	lead    *Circle
	members []*Circle
}

func (g *Group) Split() (*Circle, []*Circle, error) {
	return g.lead, g.members, nil
}

func (g *Group) Lead() float64 {
	return g.lead.Area()
}

func (g *Group) Sum() float64 {
	total := 0.0
	for _, member := range g.members {
		total += member.Area()
	}
	return total
}

func (g *Group) First() float64 {
	first, _, _ := g.Split()
	return first.Area()
}

func (g *Group) Head() float64 {
	return g.members[0].Area()
}
