import { Repo } from "./repo";

export function run(repo: Repo): string {
  return repo.find(1, true);
}
