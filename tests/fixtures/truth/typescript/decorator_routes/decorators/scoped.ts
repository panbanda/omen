export const GlobalScope =
  (scope: string): MethodDecorator =>
  () => {
    void scope;
  };

export const Licensed =
  (feature: string): MethodDecorator =>
  () => {
    void feature;
  };
