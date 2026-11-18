export function RestController(path: string) {
  return (target: unknown) => target;
}

export function Get(path: string) {
  return (target: unknown, key: string) => key;
}
