void run(void *x);

static void live_direct(void) {}
static void live_value(void) {}
static void dead_plain(void) {}
static void dead_caller(void) { dead_callee(); }
static void dead_callee(void) {}
static void dead_unresolved(void) {}

void live_public(void) {}

int main(void) {
    live_direct();
    void (*handler)(void) = live_value;
    handler();
    run(0);
    return 0;
}
