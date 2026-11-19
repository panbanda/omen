import { Container } from "./di";
import { Repo } from "./services";

export function byClass(): number {
  return Container.get(Repo).plain();
}

export function byFactory(): number {
  return Container.make(() => new Repo()).plain();
}
