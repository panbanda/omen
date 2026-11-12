import math

import app.other as alt
import app.util
from app import describe
from app.other import *
from app.shapes import (Circle, Shape, Square)
from app.util import helper
from app.util import twice as double


def local():
    return 4


def run():
    def inner():
        return 5

    total = local() + helper() + double(2) + inner() + only_here()
    circle = Circle(1.0)
    circle.area()
    numbers = list(map(lambda n: twice_local(n), [1, 2]))
    add = lambda n: n + 1
    return total + add(len(numbers))


def twice_local(value):
    return value * 2


def measure(size: Square):
    return size.area()


def inherited(size: Square):
    return size.label()


def shadow():
    helper = lambda: 9
    return helper()


def dynamic(shape):
    return shape.area()


def external():
    seen = []
    seen.insert(0, math.floor(2.5))
    return len(seen)


def qualified():
    return app.util.twice(2) + alt.helper()


def main():
    size = measure(Square.from_side(2.0))
    unit = Circle.unit()
    assert isinstance(unit, Shape)
    print(run(), shadow(), describe(), size, dynamic(unit), external(), qualified(), inherited(Square(1.0)))


if __name__ == "__main__":
    main()
