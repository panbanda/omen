import { readFileSync } from "node:fs";
import { fetch } from "./local.jsx";

export function globals() {
  console.log("x");
  parseInt("3");
}

export function maths() {
  return Math.max(1, 2);
}

export function imported() {
  readFileSync("a");
  fetch("u");
}

export function parseFloat(text) {
  return text.length;
}

export function shadowed() {
  return parseFloat("1");
}

export function parameter(console) {
  console.log("x");
}
