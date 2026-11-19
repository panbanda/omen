import { libFn } from "my-lib";
import { utilFn } from "@acme/util/strings";
import { Svc } from "@/svc/svc";

export function main(): number {
  return libFn() + utilFn() + new Svc().work();
}
