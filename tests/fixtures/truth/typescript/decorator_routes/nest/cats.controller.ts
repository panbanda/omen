import { All, Controller, Get, Post, UseGuards } from '@nestjs/common';
import { exec } from 'child_process';

class AuthGuard {}
class RolesGuard {}

@UseGuards(RolesGuard)
@Controller('cats')
export class CatsController {
  @Get(':id')
  find(): number {
    return 1;
  }

  @UseGuards(AuthGuard)
  @Post()
  run(command: string): void {
    exec(command);
  }

  @All(['a', 'b'])
  any(): void {}
}
