const { basename } = require('node:path');
const other = require('./other.js');

function legacyName() {
  return basename('a/b') + other.onlyHere();
}

module.exports = { legacyName };
