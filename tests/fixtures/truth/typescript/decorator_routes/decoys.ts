import { Controller } from '@nestjs/common';
import { Put } from 'some-library';

import { Get } from './cache';

@Controller('decoys')
export class DecoyController {
  @Get('/cached')
  cached(): void {}

  @Put('/external')
  external(): void {}

  // @Get('/comment')
  comment(): string {
    return "@Post('/string')";
  }
}
