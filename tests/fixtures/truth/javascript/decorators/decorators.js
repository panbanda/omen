export function RestController(path) {
  return (target) => target;
}

export function Get(path) {
  return (target, key) => key;
}
