#include <stdio.h>
#include <stdlib.h>

int add(int a, int b) {
    return a + b;
}

int subtract(int a, int b) {
    return a - b;
}

double calculate_area(double width, double height) {
    return width * height;
}

void print_result(int value) {
    printf("Result: %d\n", value);
}
