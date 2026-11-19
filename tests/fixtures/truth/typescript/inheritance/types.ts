export interface Hooks {
  read(name: string): string;
}

export abstract class Base {
  read(name: string): string {
    return name;
  }
}

export class Hook extends Base implements Hooks {}

export class Failure extends Error {
  constructor(message: string) {
    super(message);
  }
}

export function createNode(options: { run(): void }) {
  return class {
    run(): void {
      options.run();
    }
  };
}
