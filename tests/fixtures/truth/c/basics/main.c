#include <stdio.h>
#include <string.h>

#include "other.h"
#include "shapes.h"
#include "sub/util.h"

#define SQUARE(x) ((x) * (x))

struct Table {
    int (*insert)(int);
};

static int tag(void) {
    return 1;
}

static int local(void) {
    return 4;
}

static int twice_local(int value) {
    return value * 2;
}

static int apply(const int *values, int count, int (*f)(int)) {
    int total = 0;
    for (int i = 0; i < count; i++) {
        total += f(values[i]);
    }
    return total;
}

static int run(void) {
    const int values[] = {1, 2};
    int total = local() + helper() + only_here() + tag();
    return total + apply(values, 2, twice_local);
}

static double measure(const struct Square *size) {
    return square_area(size);
}

static int shadow(void) {
    int (*helper)(void) = only_here;
    return helper();
}

static int dynamic(int flag) {
    int (*pick)(void) = flag ? helper : only_here;
    return pick();
}

static int external(void) {
    struct Table table = {twice};
    return table.insert(3) + (int)strlen("x") + SQUARE(2);
}

int main(void) {
    struct Circle circle = {1.0};
    struct Square square = {2.0};
    enum Unit unit = METRE;
    printf("%s %f %f\n", describe(), circle_area(&circle), measure(&square));
    printf("%d %d %d %d %d\n", run(), shadow(), dynamic(unit == FOOT), external(), other_tag());
    return 0;
}
