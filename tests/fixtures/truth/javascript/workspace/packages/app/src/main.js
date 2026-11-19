import { libFn } from "my-lib";
import { Svc } from "@/svc/svc.js";

export function main() {
  return libFn() + new Svc().work();
}
