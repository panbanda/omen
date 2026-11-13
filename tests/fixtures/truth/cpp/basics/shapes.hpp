#ifndef SHAPES_HPP
#define SHAPES_HPP

#include <string>

using Scale = double;

enum class Unit {
    Metre,
    Foot
};

struct Point {
    double x;
    double y;
};

class Shape {
public:
    virtual ~Shape() = default;
    virtual double area() const = 0;
};

class Circle : public Shape {
public:
    explicit Circle(double radius);
    ~Circle() override;
    double area() const override;
    double area(Scale factor) const;
    double radius() const {
        return radius_;
    }

protected:
    double scaled(Scale factor) const;

private:
    double radius_;
};

class Square : public Shape {
public:
    explicit Square(double side) : side_(side) {
    }
    double area() const override {
        return side_ * side_;
    }

private:
    double side_;
};

std::string describe();

class Pair {
public:
    explicit Pair(Circle* lead) : lead_(lead), next_(nullptr), rest_{lead, lead} {
    }
    double lead_area() const {
        return lead_->area();
    }
    double next_area() const {
        return this->next_->lead_->area();
    }
    double rest_area() const {
        double total = 0.0;
        for (const auto* member : rest_) {
            total += member->area();
        }
        return total;
    }

private:
    Circle* lead_;
    Pair* next_;
    Circle* rest_[2];
};

#endif
