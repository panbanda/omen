#include <algorithm>
#include <cstdio>
#include <vector>
#include "util.hpp"

int helper() { return 1; }

void run(std::vector<int> numbers) {
    numbers.push_back(1);
    std::sort(numbers.begin(), numbers.end());
    printf("x");
    helper();
}

int decoy(const char *text) {
    return strlen(text);
}
