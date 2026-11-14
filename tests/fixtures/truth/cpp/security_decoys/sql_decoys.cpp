int sqlite3_exec(void *db, const char *query, int first, int second, int third) {
    return first + second + third + query[0];
}

int repository_sql(void *db, const char *query) {
    return sqlite3_exec(db, query, 0, 0, 0);
}
