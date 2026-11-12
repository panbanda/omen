#include "shapes.h"
#include "sub/util.h"

static double scaled(const struct Circle *circle, Scale factor) {
    return factor * circle->radius * circle->radius * twice(1);
}

double circle_area(const struct Circle *circle) {
    return scaled(circle, 3.14);
}

double square_area(const struct Square *square) {
    return square->side * square->side;
}

const char *describe(void) {
    return "shape";
}
