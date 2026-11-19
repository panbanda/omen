import { readFileSync } from "node:fs";
import { fetch } from "./local";

export function globals(): void {
  console.log("x");
  parseInt("3");
}

export function maths(): number {
  return Math.max(1, 2);
}

export function typed(): void {
  const cache: Map<string, number> = new Map();
  cache.set("a", 1);
}

export function imported(): void {
  readFileSync("a");
  fetch("u");
}

export function parseFloat(text: string): number {
  return text.length;
}

export function shadowed(): number {
  return parseFloat("1");
}

export function parameter(console): void {
  console.log("x");
}

export function collect(): number {
  const seen = new Set<string>();
  seen.add("a");
  return new Date().getTime() + seen.size;
}
