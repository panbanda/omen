#include <string>
#include <vector>

class Shape {
public:
    double width;
    double height;

    double area() {
        return width * height;
    }

    double perimeter() {
        return 2 * (width + height);
    }
};

int add(int a, int b) {
    return a + b;
}

std::string format_label(const std::string& name, int count) {
    return name + ": " + std::to_string(count);
}
