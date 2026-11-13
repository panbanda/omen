export function helper(): number {
  return 1;
}

export function twice(value: number): number {
  return value * 2;
}

export const triple = (value: number): number => value * 3;

export function format(value: number): string;
export function format(value: string): string;
export function format(value: number | string): string {
  return String(value);
}

export default function fallback(): number {
  return 7;
}
