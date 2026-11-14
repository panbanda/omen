#include <sqlite3.h>
#include <stdio.h>

void sql_string_construction(sqlite3 *db, const char *name) {
    char query[256];
    snprintf(query, sizeof query, "SELECT * FROM users WHERE name = '%s'", name);
    sqlite3_exec(db, query, 0, 0, 0);
    sqlite3_exec(db, "SELECT 1", 0, 0, 0);
    const char *text = "sqlite3_exec(db, query)";
    /* sqlite3_exec(db, query) */
}
