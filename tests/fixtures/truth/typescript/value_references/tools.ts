function supportsModel(model: string): boolean {
  return model.length > 0;
}

function prepareTools(): number {
  return 1;
}

export function makeDeferral() {
  return { supportsModel, prepare: prepareTools };
}

export function register(list: Array<() => number>): void {
  list.push(prepareTools);
}
