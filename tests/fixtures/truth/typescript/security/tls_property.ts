import { Pool } from "pg";

export function tlsOptions(allow: boolean, options: { rejectUnauthorized?: boolean }) {
  new Pool({ ssl: { rejectUnauthorized: false } });
  const settings = { rejectUnauthorized: !allow };
  const safe = { rejectUnauthorized: true };
  const { rejectUnauthorized } = options;
  process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0";
  return [settings, safe, rejectUnauthorized];
}
