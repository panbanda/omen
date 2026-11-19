import { Svc } from "@/svc/svc";

export function page(): number {
  return new Svc().work();
}
