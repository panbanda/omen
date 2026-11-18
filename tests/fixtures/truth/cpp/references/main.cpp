#include <cstdio>
#include <cstdlib>
#include <thread>

extern void (*cleanup)();

static void audit() {
    system(getenv("AUDIT"));
}

struct Job {
    void step() {
        system(getenv("STEP"));
    }

    void start() {
        std::thread worker(&Job::step, this);
        worker.join();
    }
};

static void shadowed(void (*audit)()) {
    atexit(audit);
}

static void text_only() {
    // atexit(audit);
    puts("audit");
}

int main(int argc, char **argv) {
    auto load = [argv]() { system(argv[1]); };
    std::thread second{load};
    second.join();
    atexit(audit);
    atexit(cleanup);
    Job job;
    job.start();
    shadowed(nullptr);
    text_only();
    return 0;
}
