#include <foreign.h>

void run(void *p) {
    struct foreign *x = p;
    x->dead_unresolved();
}
