import { basename } from 'node:path';
import * as os from 'node:os';
import { Circle, Square, describe } from './shapes.js';
import { helper } from './util.js';
import { twice as double } from './util.js';
import fallback from './util.js';
import * as other from './other.js';
import { onlyHere } from './barrel.js';
import legacy from './legacy.cjs';

const registry = {
  count: 0,
  bump() {
    return this.count + 1;
  },
  reset: () => 0,
};

function local() {
  return 4;
}

function run() {
  function inner() {
    return 5;
  }
  const add = (n) => n + 1;
  const sub = function (n) {
    return n - 1;
  };
  const total = local() + helper() + double(2) + inner() + onlyHere() + fallback();
  const circle = new Circle(1);
  circle.area();
  const numbers = [1, 2].map((n) => twiceLocal(n));
  return total + add(numbers.length) + sub(1);
}

function twiceLocal(value) {
  return value * 2;
}

function namespaced() {
  return other.helper();
}

function makeSquare() {
  return new Square();
}

function unit() {
  return Circle.unit().radius;
}

function useRegistry() {
  return registry.bump() + registry.reset();
}

function shadow() {
  const helper = () => 9;
  return helper();
}

function dynamic(shape) {
  return shape.area();
}

function external() {
  const seen = new Map();
  seen.set('a', 1);
  return parseInt('4', 10) + seen.size;
}

function fileName() {
  return basename('a/b') + os.EOL + legacy.legacyName();
}

function main() {
  const size = dynamic(makeSquare());
  console.log(run(), shadow(), describe(), size, dynamic(new Circle(1)));
  console.log(external(), fileName(), namespaced(), unit(), useRegistry());
}

main();
