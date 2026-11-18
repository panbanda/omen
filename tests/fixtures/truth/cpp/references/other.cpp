#include <cstdlib>

static void cleanup() {
    system(getenv("TARGET"));
}

void run_cleanup() {
    cleanup();
}
