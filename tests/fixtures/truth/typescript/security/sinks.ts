import * as childProcess from "child_process";
import crypto from "crypto";
import https from "https";
import { createCipheriv as cipher, createHash as hash } from "crypto";
import { exec as run } from "child_process";
import { Agent as SecureAgent } from "https";
import { unserialize as revive } from "node-serialize";
import { runInNewContext as evaluate } from "vm";
import serialize from "node-serialize";

export function commandExecution(command) {
  childProcess.exec(command);
  run(command);
  childProcess.execFile("ls", ["-l"]);
  const text = "childProcess.exec(command)";
  // childProcess.exec(command)
  return text;
}

export function codeEvaluation(expression) {
  eval(expression);
  evaluate(expression, {});
  JSON.parse(expression);
  const text = "eval(expression)";
  // eval(expression)
  return text;
}

export function unsafeDeserialization(blob) {
  serialize.unserialize(blob);
  revive(blob);
  JSON.parse(blob);
  const text = "serialize.unserialize(blob)";
  // serialize.unserialize(blob)
  return text;
}

export function sqlStringConstruction(db, name) {
  db.query("SELECT * FROM users WHERE name = '" + name + "'");
  db.query("SELECT * FROM users WHERE name = $1", [name]);
  const text = "db.query(statement)";
  // db.query(statement)
  return text;
}

export function weakHash(data) {
  crypto.createHash("md5");
  hash("sha1");
  crypto.createHash("sha256");
  const text = "crypto.createHash('md5')";
  // crypto.createHash("md5")
  return text;
}

export function weakCipher(key) {
  crypto.createCipheriv("des-ecb", key, null);
  cipher("rc4", key, null);
  crypto.createCipheriv("aes-256-gcm", key, null);
  const text = "crypto.createCipheriv('des-ecb')";
  // crypto.createCipheriv("des-ecb", key, null)
  return text;
}

export function tlsVerificationDisabled(host) {
  https.request({ host, rejectUnauthorized: false });
  new SecureAgent({ rejectUnauthorized: false });
  https.request({ host, rejectUnauthorized: true });
  const text = "https.request({ rejectUnauthorized: false })";
  // https.request({ rejectUnauthorized: false })
  return text;
}

export function hardcodedCredential() {
  const token = "AKIAJ3KD92LQ7MZP5XWB";
  const name = "AKIAEXAMPLE";
  // AKIAJ3KD92LQ7MZP5XWB
  return [token, name];
}
