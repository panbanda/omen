#include <sqlite3.h>

void test_sql(sqlite3 *db, const char *query) {
    sqlite3_exec(db, query, 0, 0, 0);
}
