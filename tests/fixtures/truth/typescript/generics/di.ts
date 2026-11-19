export type Constructable<T = unknown> = new (...args: any[]) => T;
export type AbstractConstructable<T = unknown> = abstract new (...args: any[]) => T;
export type ServiceIdentifier<T = unknown> = Constructable<T> | AbstractConstructable<T>;

const instances = new Map<ServiceIdentifier, unknown>();

class ContainerClass {
  get<T>(type: ServiceIdentifier<T>): T {
    return instances.get(type) as T;
  }

  make<T>(factory: () => T): T {
    return factory();
  }
}

export const Container = new ContainerClass();
