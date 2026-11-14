import https from "./helpers";
import serialize from "./helpers";
import crypto from "./helpers";
import { Gateway, createCipheriv, exec, runInNewContext } from "./helpers";

export function repositorySymbols(command, name) {
  exec(command);
  runInNewContext(command);
  serialize.unserialize(command);
  const gateway = new Gateway();
  gateway.query("SELECT * FROM users WHERE name = '" + name + "'");
  crypto.createHash("md5");
  createCipheriv("des-ecb");
  https.request({ rejectUnauthorized: false });
}
