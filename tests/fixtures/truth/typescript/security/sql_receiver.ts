import type { SimpleGit } from "simple-git";
import type { Knex } from "knex";

export async function sqlReceivers(git: SimpleGit, knex: Knex, name: string) {
  await git.raw(`log ${name}`);
  await knex.raw(`SELECT * FROM users WHERE name = '${name}'`);
  await knex.raw(`SELECT * FROM ${"users"}`);
  await knex.raw(["SELECT", `${name}`]);
}
