#include <criterion/criterion.h>

int production(void);

static int helper(void) {
    return production();
}

Test(service, gives_one) {
    int value = helper();
    cr_assert_eq(value, 1);
}
