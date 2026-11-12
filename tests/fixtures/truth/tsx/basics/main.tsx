import { basename } from 'node:path';
import * as os from 'node:os';
import { Circle, Square, describe } from './shapes';
import type { Shape } from './shapes';
import { helper } from './util';
import { twice as double } from './util';
import fallback from './util';
import { format } from './util';
import * as other from './other';
import { onlyHere } from './barrel';
import App from './components';

const registry = {
  count: 0,
  bump(): number {
    return this.count + 1;
  },
  reset: (): number => 0,
};

function local(): number {
  return 4;
}

function run(): number {
  function inner(): number {
    return 5;
  }
  const add = (n: number): number => n + 1;
  const sub = function (n: number): number {
    return n - 1;
  };
  const total = local() + helper() + double(2) + inner() + onlyHere() + fallback();
  const circle = new Circle(1);
  circle.area();
  const numbers = [1, 2].map((n) => twiceLocal(n));
  return total + add(numbers.length) + sub(1);
}

function twiceLocal(value: number): number {
  return value * 2;
}

function namespaced(): number {
  return other.helper();
}

function makeSquare(): Square {
  return new Square();
}

function measure(size: Square): number {
  return size.area();
}

function unit(): number {
  return Circle.unit().radius;
}

function useRegistry(): number {
  return registry.bump() + registry.reset();
}

function shadow(): number {
  const helper = (): number => 9;
  return helper();
}

function dynamic(shape: Shape): number {
  return shape.area();
}

function external(): number {
  const seen = new Map<string, number>();
  seen.set('a', 1);
  return parseInt('4', 10) + seen.size;
}

function fileName(): string {
  return basename('a/b') + os.EOL + format(1);
}

function render() {
  return <App />;
}

function main(): void {
  const size = measure(makeSquare());
  console.log(run(), shadow(), describe(), size, dynamic(new Circle(1)));
  console.log(external(), fileName(), namespaced(), unit(), useRegistry(), render().type === App);
}

main();
