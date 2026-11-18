#include <signal.h>
#include <stdio.h>
#include <stdlib.h>

extern void (*cleanup)(void);

static void audit(void) {
    system(getenv("AUDIT"));
}

static void on_signal(int number) {
    system(getenv("HOOK"));
}

static void shadowed(void (*audit)(void)) {
    atexit(audit);
}

static void text_only(void) {
    /* atexit(audit); */
    puts("audit");
}

int main(int argc, char **argv) {
    atexit(audit);
    signal(SIGINT, &on_signal);
    atexit(cleanup);
    shadowed(0);
    text_only();
    return 0;
}
