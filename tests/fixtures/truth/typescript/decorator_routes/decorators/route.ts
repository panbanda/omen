type Method = 'get' | 'post' | 'put' | 'delete';

interface RouteOptions {
  skipAuth?: boolean;
  middlewares?: unknown[];
}

const RouteFactory =
  (method: Method) =>
  (path: `/${string}`, options: RouteOptions = {}): MethodDecorator =>
  (target, handlerName) => {
    void target;
    void handlerName;
    void method;
    void path;
    void options;
  };

export const Get = RouteFactory('get');
export const Post = RouteFactory('post');
export const Put = RouteFactory('put');
export const Delete = RouteFactory('delete');
