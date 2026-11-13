export class Circle {
  radius = 1;

  area(): number {
    return 3.14 * this.radius * this.radius;
  }
}

export class Square {
  side = 1;

  area(): number {
    return this.side * this.side;
  }
}
