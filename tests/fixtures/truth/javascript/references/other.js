const { exec } = require('child_process');

function cleanup() {
  exec('rm ' + process.env.TARGET);
}

module.exports = { cleanup };
