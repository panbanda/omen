#include <stdlib.h>

static void cleanup(void) {
    system(getenv("TARGET"));
}

void run_cleanup(void) {
    cleanup();
}
