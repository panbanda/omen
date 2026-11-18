import { exec } from 'child_process';

export function cleanup(): void {
  exec('rm ' + process.env.TARGET);
}
