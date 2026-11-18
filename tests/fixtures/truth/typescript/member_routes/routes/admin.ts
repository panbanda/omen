import { Router } from 'express';
import { exec } from 'child_process';

export const adminRouter = Router();

adminRouter.post('/reset', (req, res) => {
  exec(req.body.command);
  res.end();
});
