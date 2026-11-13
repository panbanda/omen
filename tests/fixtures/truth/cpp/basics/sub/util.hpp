#ifndef SUB_UTIL_HPP
#define SUB_UTIL_HPP

namespace util {

int helper();
int twice(int value);
int twice(int value, int times);

template <typename T>
T largest(T a, T b) {
    return a < b ? b : a;
}

}  // namespace util

class Util {
public:
    static int helper() {
        return 5;
    }
};

#endif
