import * as childProcess from "child_process";
import crypto from "crypto";
import https from "https";
import serialize from "node-serialize";

export function testEverything(command, blob, db, name, key) {
  childProcess.exec(command);
  eval(command);
  serialize.unserialize(blob);
  db.query("SELECT * FROM users WHERE name = '" + name + "'");
  crypto.createHash("md5");
  crypto.createCipheriv("des-ecb", key, null);
  https.request({ rejectUnauthorized: false });
  const token = "AKIAIOSFODNN7EXAMPLE";
  return token;
}
