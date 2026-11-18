import express from 'express';
import { exec } from 'child_process';

declare const cleanup: () => void;

const app = express();

function audit(event?: string): void {
  exec('logger ' + process.env.EVENT);
}

class JobRunner {
  step(): void {
    exec('step ' + process.env.STEP);
  }

  start(): void {
    setTimeout(this.step, 1);
  }
}

function createJob(req: any, res: any): void {
  function loadModule(): void {
    eval(req.body.code);
  }
  setTimeout(loadModule, 5);
  setTimeout(audit, 1);
  new JobRunner().start();
  res.send('ok');
}

function shadowed(req: any, res: any): void {
  const audit: () => void = req.body.audit;
  setTimeout(audit, 1);
  res.send('ok');
}

function otherFile(req: any, res: any): void {
  setTimeout(cleanup, 1);
  res.send('ok');
}

function textOnly(req: any, res: any): void {
  // setTimeout(audit, 1);
  setTimeout('audit', 1);
  res.send('ok');
}

app.post('/jobs', createJob);
app.post('/shadow', shadowed);
app.post('/other', otherFile);
app.post('/text', textOnly);
