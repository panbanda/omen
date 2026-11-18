import { exec, execFile, spawn } from "node:child_process";
import { promisify } from "node:util";

const asyncExec = promisify(exec);

export async function shellCommands(command: string, program: string) {
  execFile("sh", ["-c", command]);
  spawn("git", ["-c", command]);
  await asyncExec(command);
  spawn(program, ["-x"]);
}
