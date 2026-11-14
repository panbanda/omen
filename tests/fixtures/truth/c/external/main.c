#include <stdio.h>
#include <string.h>
#include "util.h"

int helper(void) { return 1; }

int run(char *text) {
    printf("x");
    strlen(text);
    helper();
    return 0;
}

int decoy(void) {
    return atoi("1");
}
