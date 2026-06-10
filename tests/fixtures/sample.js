import { EventEmitter } from 'events';
import { readFileSync } from 'fs';

class EventBus {
  constructor() {
    this.listeners = {};
  }

  subscribe(event, handler) {
    if (!this.listeners[event]) {
      this.listeners[event] = [];
    }
    this.listeners[event].push(handler);
  }

  publish(event, data) {
    const handlers = this.listeners[event] || [];
    handlers.forEach(h => h(data));
  }
}

function parseConfig(raw) {
  const result = {};
  for (const line of raw.split('\n')) {
    if (line.includes('=')) {
      const [key, ...rest] = line.split('=');
      result[key.trim()] = rest.join('=').trim();
    }
  }
  return result;
}

function formatError(err) {
  return `[ERROR] ${err.message}`;
}
