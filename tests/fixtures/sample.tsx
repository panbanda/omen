import React, { useState } from 'react';
import { StyleSheet } from 'react-native';

export class ThemeProvider {
  private theme: string;

  constructor(theme: string) {
    this.theme = theme;
  }

  getColor(key: string): string {
    return key === 'primary' ? '#007bff' : '#6c757d';
  }

  getSpacing(size: number): number {
    return size * 8;
  }
}

export function Button({ label, onClick }: { label: string; onClick: () => void }) {
  return <button onClick={onClick}>{label}</button>;
}

export function formatLabel(text: string, count: number): string {
  return `${text} (${count})`;
}
