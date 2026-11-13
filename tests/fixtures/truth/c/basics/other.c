#include "other.h"

static int tag(void) {
    return 2;
}

int only_here(void) {
    return 3;
}

int insert(int value) {
    return value;
}

int other_tag(void) {
    return tag();
}
