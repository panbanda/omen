#include <algorithm>
#include <cmath>
#include <iostream>
#include <vector>

#include "other.hpp"
#include "shapes.hpp"
#include "sub/util.hpp"

using namespace other;
namespace tools = util;

namespace {

int local() {
    return 4;
}

}  // namespace

static int twice_local(int value) {
    return value * 2;
}

int run() {
    int total = local() + util::helper() + only_here();
    auto circle = Circle(1.0);
    total += static_cast<int>(circle.area());
    std::vector<int> numbers = {1, 2};
    std::transform(numbers.begin(), numbers.end(), numbers.begin(), [](int n) { return twice_local(n); });
    auto add = [](int n) { return n + 1; };
    return total + add(numbers[0]);
}

int overload_one() {
    return util::twice(2);
}

int overload_two() {
    return util::twice(2, 3);
}

int aliased() {
    return tools::helper();
}

int qualified() {
    return Util::helper();
}

double measure(const Circle& size) {
    return size.area();
}

double measure_scaled(const Circle* size) {
    return size->area(2.0);
}

double measure_square(const Square& size) {
    return size.area();
}

double dynamic(const Shape* shape) {
    return shape->area();
}

double braces() {
    Circle made = Circle{2.0};
    return made.radius();
}

double heap() {
    Circle* made = new Circle(1.0);
    double radius = made->radius();
    delete made;
    return radius;
}

double declared() {
    Circle circle(1.0);
    return measure(circle) + measure_scaled(&circle);
}

double squares() {
    Square square(2.0);
    return measure_square(square) + dynamic(&square);
}

int shadow() {
    auto helper = []() { return 9; };
    return helper();
}

int external() {
    std::vector<int> seen;
    seen.push_back(1);
    return static_cast<int>(std::sqrt(4.0)) + static_cast<int>(seen.size());
}

int main() {
    Point origin{0.0, 0.0};
    Unit unit = Unit::Metre;
    std::cout << describe() << " " << external() << " " << run() << " " << shadow() << "\n";
    std::cout << overload_one() << " " << overload_two() << " " << aliased() << " " << qualified() << "\n";
    std::cout << braces() << " " << heap() << " " << declared() << " " << squares() << "\n";
    std::cout << util::largest(1, 2) << " " << origin.x << " " << (unit == Unit::Foot) << "\n";
    return 0;
}
