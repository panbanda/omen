const express = require('express');
const { exec } = require('child_process');

const app = express();

function audit(event) {
  exec('logger ' + process.env.EVENT);
}

function createJob(req, res) {
  function loadModule() {
    eval(req.body.code);
  }
  setTimeout(loadModule, 5);
  setTimeout(audit, 1);
  res.send('ok');
}

function shadowed(req, res) {
  const audit = req.body.audit;
  setTimeout(audit, 1);
  res.send('ok');
}

function otherFile(req, res) {
  setTimeout(cleanup, 1);
  res.send('ok');
}

function textOnly(req, res) {
  // setTimeout(audit, 1);
  setTimeout('audit', 1);
  res.send('ok');
}

app.post('/jobs', createJob);
app.post('/shadow', shadowed);
app.post('/other', otherFile);
app.post('/text', textOnly);
