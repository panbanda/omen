import { run } from "./other.js";

class Tool {
  liveMethod() {}
  deadMethod() {}
}

function liveDirect() {}
function liveValue() {}
function deadPlain() {}
function deadCaller() {
  deadCallee();
}
function deadCallee() {}
function deadUnresolved() {}

export function livePublic() {}

function main() {
  liveDirect();
  const handler = liveValue;
  handler();
  const tool = new Tool();
  tool.liveMethod();
  run(undefined);
}

main();
