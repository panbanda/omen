void run(void *x);

class Tool {
public:
    void liveMethod() {}

private:
    void deadMethod() {}
};

static void liveDirect() {}
static void liveValue() {}
static void deadPlain() {}
static void deadCaller() { deadCallee(); }
static void deadCallee() {}
static void deadUnresolved() {}

void livePublic() {}

int main() {
    liveDirect();
    void (*handler)() = liveValue;
    handler();
    Tool tool;
    tool.liveMethod();
    run(nullptr);
    return 0;
}
