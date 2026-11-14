import hashlib

from psycopg import sql


def declared_safe(cursor, data, name):
    cursor.execute(sql.SQL("DROP TABLE {}").format(sql.Identifier(name)))
    hashlib.md5(data, usedforsecurity=False)
    hashlib.new("md5", data, usedforsecurity=False)
    return "AKIAIOSFODNN7EXAMPLE"
