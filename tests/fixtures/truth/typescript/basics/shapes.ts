import { twice } from './util';

export interface Shape {
  area(): number;
}

export type Scale = number;

export enum Unit {
  Metre,
  Foot,
}

export abstract class Base {
  name: string;

  constructor(name: string) {
    this.name = name;
  }

  abstract area(): number;

  describe(): string {
    return this.name;
  }
}

export class Circle extends Base implements Shape {
  radius: number;

  constructor(radius: number) {
    super('circle');
    this.radius = radius;
  }

  area(): number {
    return this.#scaled(3);
  }

  #scaled(factor: Scale): number {
    return twice(this.radius) * factor;
  }

  override describe(): string {
    return super.describe() + '!';
  }

  static unit(): Circle {
    return new Circle(1);
  }

  grow = (): number => this.radius + 1;
}

export class Square implements Shape {
  side = 2;

  area(): number {
    return this.doubled() / 2;
  }

  private doubled(): number {
    return this.side * this.side * 2;
  }
}

export function describe(): string {
  return 'shapes';
}

function hidden(): Unit {
  return Unit.Metre;
}

class Pair {
  private lead: Circle;
  private rest: Circle[];

  constructor(lead: Circle) {
    this.lead = lead;
    this.rest = [];
  }

  leadArea(): number {
    return this.lead.area();
  }

  restArea(): number {
    let total = 0;
    for (const member of this.rest) {
      total += member.area();
    }
    return total;
  }
}
