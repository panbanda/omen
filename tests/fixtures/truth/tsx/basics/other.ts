export function helper(): number {
  return 2;
}

export function onlyHere(): number {
  return 3;
}

export function set(key: string, value: number): number {
  return key.length + value;
}
