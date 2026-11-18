export const RestController =
  (basePath: `/${string}` = '/'): ClassDecorator =>
  (target) => {
    void basePath;
    return target;
  };
