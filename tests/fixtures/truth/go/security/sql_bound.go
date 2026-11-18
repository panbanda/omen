package main

import (
	"database/sql"
	"fmt"

	"xorm.io/xorm"
)

func boundParameters(db *sql.DB, ctx int, value string) {
	db.Exec("INSERT INTO users VALUES (?)", fmt.Sprintf("%s", value))
	db.ExecContext(ctx, "INSERT INTO users VALUES (?)", fmt.Sprintf("%s", value))
	db.ExecContext(ctx, fmt.Sprintf("INSERT INTO users VALUES ('%s')", value))
}

func ormQueries(engine *xorm.Engine, db *sql.DB, table string, id int) {
	engine.SQL(fmt.Sprintf("SELECT max_index FROM %s WHERE group_id = ?", table), id)
	db.Query("SELECT max_index "+
		"FROM users WHERE id = ?", id)
	engine.Get(fmt.Sprintf("key-%s", table))
}
