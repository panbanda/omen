import { RestController, Get } from "./decorators.js";

@RestController("/x")
export class Controller {
  @Get("/list")
  list() {
    return 1;
  }
}
