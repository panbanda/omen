const express = require('express');
const { exec } = require('child_process');

const app = express();

function runCommand(req, res) {
  prepare(req.query.x);
  res.send('ok');
}

function prepare(value) {
  execute(value);
}

function execute(value) {
  exec('echo ' + value);
}

function orphan(expression) {
  eval(expression);
}

app.get('/run', requireAuth, runCommand);
