export class Repo {
  find(id: string): string;
  find(id: number, all: boolean): string;
  find(id: unknown, all?: boolean): string {
    return String(id) + String(all);
  }

  first(): string {
    return this.find("a");
  }
}
