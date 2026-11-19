export function Get(key: string) {
  return (target: unknown, name: string) => {
    void key;
    void target;
    void name;
  };
}
