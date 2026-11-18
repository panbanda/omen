export function production() {
  return 1;
}

export function testConnection() {
  return production();
}

export function iterate(it) {
  return it.next();
}

describe("production", () => {
  function helper() {
    return production();
  }

  beforeEach(() => {
    helper();
  });

  it("gives one", () => {
    const value = helper();
    expect(value).toBe(1);
  });

  test.each([1, 2])("accepts %s", (input) => {
    expect(input + production()).toBeGreaterThan(1);
  });
});
