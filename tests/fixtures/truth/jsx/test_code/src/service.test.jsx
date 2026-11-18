import { production } from "./service";

function sharedHelper() {
  return production();
}

test("uses the helper", () => {
  expect(sharedHelper()).toBe(1);
});

function Badge() {
  return <span>{sharedHelper()}</span>;
}

it("renders the badge", () => {
  expect(<Badge />).toBeTruthy();
});
