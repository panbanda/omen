#include "shapes.hpp"

Circle::Circle(double radius) : radius_(radius) {
}

Circle::~Circle() {
}

double Circle::area() const {
    return this->scaled(3.14);
}

double Circle::area(Scale factor) const {
    return scaled(factor);
}

double Circle::scaled(Scale factor) const {
    return factor * radius_ * radius_;
}

std::string describe() {
    return "shape";
}
