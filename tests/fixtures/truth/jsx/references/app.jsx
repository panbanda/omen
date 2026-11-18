const express = require('express');
const { exec } = require('child_process');
const { renderToString } = require('react-dom/server');

const app = express();

function audit(event) {
  exec('logger ' + process.env.EVENT);
}

function page(req, res) {
  function handleClick() {
    eval(req.body.code);
  }
  res.send(renderToString(<Button onClick={handleClick} onKey={audit} />));
}

function shadowed(req, res) {
  const audit = req.body.audit;
  res.send(renderToString(<Button onClick={audit} />));
}

function otherFile(req, res) {
  res.send(renderToString(<Button onClick={cleanup} />));
}

function textOnly(req, res) {
  // <Button onClick={audit} />
  res.send(renderToString(<Button onClick="audit" label="audit" />));
}

app.post('/page', page);
app.post('/shadow', shadowed);
app.post('/other', otherFile);
app.post('/text', textOnly);
