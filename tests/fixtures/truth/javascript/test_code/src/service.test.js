import { production } from "./service";

function sharedHelper() {
  return production();
}

test("uses the helper", () => {
  expect(sharedHelper()).toBe(1);
});
