#include <stdlib.h>

static void execute(const char *value) {
    system(value);
}

static void prepare(const char *value) {
    execute(value);
}

static void orphan(const char *value) {
    system(value);
}

int main(int argc, char **argv) {
    prepare(argv[1]);
    return 0;
}
