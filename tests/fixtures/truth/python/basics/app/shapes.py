import math
from typing import TypeAlias

from .util import twice

Scale: TypeAlias = float


class Shape:
    def __init__(self, name):
        self.name = name

    def area(self):
        raise NotImplementedError

    def label(self):
        return self.name


class Circle(Shape):
    def __init__(self, radius):
        super().__init__("circle")
        self.radius = radius

    def area(self):
        return self._scaled(math.pi)

    def _scaled(self, factor: Scale):
        return factor * self.radius * self.radius * twice(1)

    @staticmethod
    def unit():
        return Circle(1.0)


class Square(Shape):
    def __init__(self, side):
        super().__init__("square")
        self.side = side

    def area(self):
        return self.side * self.side

    @classmethod
    def from_side(cls, side):
        return cls(side)


def describe():
    return "shape"


def _hidden():
    return 7


class Pair:
    lead: "Circle"

    def __init__(self):
        self.spare = Circle(1.0)
        self.rest: list[Circle] = []

    def lead_area(self):
        return self.lead.area()

    def spare_area(self):
        return self.spare.area()

    def rest_area(self):
        total = 0.0
        for member in self.rest:
            total += member.area()
        return total
