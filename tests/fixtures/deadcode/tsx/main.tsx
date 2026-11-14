import { run } from "./other.tsx";

class Tool {
  liveMethod(): void {}
  private deadMethod(): void {}
}

function liveDirect(): void {}
function liveValue(): void {}
function deadPlain(): void {}
function deadCaller(): void {
  deadCallee();
}
function deadCallee(): void {}
function deadUnresolved(): void {}

export function livePublic(): void {}

function main(): void {
  liveDirect();
  const handler = liveValue;
  handler();
  const tool: Tool = new Tool();
  tool.liveMethod();
  run(undefined);
}

main();
