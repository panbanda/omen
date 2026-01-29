export interface Logger {
  info(msg: string): void;
  error(msg: string): void;
}

export class ConsoleLogger implements Logger {
  private prefix: string;

  constructor(prefix: string) {
    this.prefix = prefix;
  }

  info(msg: string): void {
    console.log(`[${this.prefix}] INFO: ${msg}`);
  }

  error(msg: string): void {
    console.error(`[${this.prefix}] ERROR: ${msg}`);
  }
}

export function parseConfig(raw: string): Record<string, string> {
  const result: Record<string, string> = {};
  for (const line of raw.split("\n")) {
    if (line.includes("=")) {
      const [key, ...rest] = line.split("=");
      result[key.trim()] = rest.join("=").trim();
    }
  }
  return result;
}
