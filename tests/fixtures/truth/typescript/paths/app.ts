import express from 'express';
import { exec } from 'child_process';

const app = express();

function runCommand(req: any, res: any): void {
  prepare(req.query.x);
  res.send('ok');
}

function prepare(value: string): void {
  execute(value);
}

function execute(value: string): void {
  exec('echo ' + value);
}

function orphan(expression: string): void {
  eval(expression);
}

app.get('/run', requireAuth, runCommand);
