import { production } from "./service";

function sharedHelper(): number {
  return production();
}

test("uses the helper", () => {
  expect(sharedHelper()).toBe(1);
});
