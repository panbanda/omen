const express = require("express");
const app = express();

function show(req, res) {}
app.get("/users/:id", authenticate, show);
app.post("/y", (req, res) => {});
