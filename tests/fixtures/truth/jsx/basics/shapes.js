import { twice } from './util';

export class Base {
  constructor(name) {
    this.name = name;
  }

  describe() {
    return this.name;
  }
}

export class Circle extends Base {
  constructor(radius) {
    super('circle');
    this.radius = radius;
  }

  area() {
    return this.#scaled(3);
  }

  #scaled(factor) {
    return twice(this.radius) * factor;
  }

  describe() {
    return super.describe() + '!';
  }

  static unit() {
    return new Circle(1);
  }

  grow = () => this.radius + 1;
}

export class Square {
  side = 2;

  area() {
    return this.side * this.side;
  }
}

export function describe() {
  return 'shapes';
}

function hidden() {
  return 0;
}
