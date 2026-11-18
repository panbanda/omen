#include "unity.h"

int production(void);

static int shared(void) {
    return production();
}

void test_production(void) {
    TEST_ASSERT_EQUAL_INT(1, shared());
}

int main(void) {
    UNITY_BEGIN();
    RUN_TEST(test_production);
    return UNITY_END();
}
