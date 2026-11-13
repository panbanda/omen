#ifndef SHAPES_H
#define SHAPES_H

struct Circle {
    double radius;
};

struct Square {
    double side;
};

enum Unit {
    METRE,
    FOOT
};

typedef double Scale;

double circle_area(const struct Circle *circle);
double square_area(const struct Square *square);
const char *describe(void);

#endif
