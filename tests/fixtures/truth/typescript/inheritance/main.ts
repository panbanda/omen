import { Hook, Failure, createNode } from "./types";

class Node extends createNode({ run() { throw new Failure("x"); } }) {}

export function run(hook: Hook): string {
  new Node();
  return hook.read("a");
}
