import { Script } from "node:vm";
import * as vm from "vm";

export function compile(code: string) {
  const script = new Script(code);
  new vm.Script(code);
  vm.createContext({});
  return script;
}
