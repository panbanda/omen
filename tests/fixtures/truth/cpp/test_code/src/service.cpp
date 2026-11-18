int production() {
    return 1;
}

int TEST_like(int value) {
    return value + production();
}

class Test {
public:
    Test() { value = production(); }

private:
    int value;
};
