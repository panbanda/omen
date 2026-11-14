#include <foreign.hpp>

void run(void *p) {
    auto *x = static_cast<foreign::Thing *>(p);
    x->deadUnresolved();
}
