import { exec } from 'child_process';

import { Delete, Get, GlobalScope, Licensed, Post, RestController } from '../decorators';

function audit(): void {}

const listMiddlewares = [audit];

function runCommand(command: string): void {
  exec(command);
}

@RestController('/workflows')
export class WorkflowsController {
  @Get('/:id')
  @GlobalScope('workflow:read')
  async one(): Promise<number> {
    return 1;
  }

  @Post('/', { skipAuth: true, middlewares: [audit] })
  @Licensed('feat:sharing')
  async create(command: string): Promise<void> {
    runCommand(command);
  }
}

@RestController()
export class RootController {
  @Delete('/cache')
  async clear(): Promise<void> {}

  @Get('/list', { middlewares: listMiddlewares })
  async list(): Promise<void> {}
}
