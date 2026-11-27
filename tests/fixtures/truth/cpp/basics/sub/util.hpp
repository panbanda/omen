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

template <>
inline const char* largest<const char*>(const char* a, const char* b) {
    return a < b ? b : a;
}

}  // namespace util

class Util {
public:
    static int helper() {
        return 5;
    }
    explicit operator bool() const {
        return true;
    }
    operator const char*() const {
        return "util";
    }
};

#endif
