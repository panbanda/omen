import * as childProcess from "child_process";

export function spawnShell() {
  childProcess.spawn("ls", [], { shell: true });
  childProcess.spawn("ls", []);
  childProcess.spawn("ls", [], { env: { shell: true } });
}
