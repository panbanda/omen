#include <gtest/gtest.h>

int production();

static int helper() {
    return production();
}

class ServiceFixture : public ::testing::Test {};

TEST(Service, GivesOne) {
    int value = helper();
    EXPECT_EQ(value, 1);
}

TEST_F(ServiceFixture, WithFixture) {
    int value = helper();
    EXPECT_EQ(value, 1);
}
