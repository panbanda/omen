import { RestController, Get } from "./decorators";

@RestController("/x")
export class Controller {
  @Get("/list")
  list(): number {
    return 1;
  }
}
