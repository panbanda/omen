package main

import (
	"database/sql"
	"fmt"
)

func boundParameters(db *sql.DB, ctx int, value string) {
	db.Exec("INSERT INTO users VALUES (?)", fmt.Sprintf("%s", value))
	db.ExecContext(ctx, "INSERT INTO users VALUES (?)", fmt.Sprintf("%s", value))
	db.ExecContext(ctx, fmt.Sprintf("INSERT INTO users VALUES ('%s')", value))
}
