import express from 'express';
import { exec } from 'child_process';
import { renderToString } from 'react-dom/server';

declare const cleanup: () => void;

const app = express();

function audit(event?: unknown): void {
  exec('logger ' + process.env.EVENT);
}

function page(req: any, res: any): void {
  const handleClick = (): void => {
    eval(req.body.code);
  };
  res.send(renderToString(<Button onClick={handleClick} onKey={audit} />));
}

function shadowed(req: any, res: any): void {
  const audit: () => void = req.body.audit;
  res.send(renderToString(<Button onClick={audit} />));
}

function otherFile(req: any, res: any): void {
  res.send(renderToString(<Button onClick={cleanup} />));
}

function textOnly(req: any, res: any): void {
  // <Button onClick={audit} />
  res.send(renderToString(<Button onClick="audit" label="audit" />));
}

app.post('/page', page);
app.post('/shadow', shadowed);
app.post('/other', otherFile);
app.post('/text', textOnly);
