import { useEffect, useState } from 'react';
import { helper } from './util';

interface BadgeProps {
  label: string;
}

export function Badge({ label }: BadgeProps) {
  return <span>{label}</span>;
}

function track(value: number): number {
  return value + helper();
}

export const Counter = () => {
  const [count, setCount] = useState(0);
  useEffect(() => {
    track(count);
  }, [count]);
  const handleClick = () => setCount(count + 1);
  return (
    <button onClick={handleClick}>
      <Badge label={String(count)} />
    </button>
  );
};

export default function App() {
  return (
    <div onClick={() => track(1)}>
      <Counter />
    </div>
  );
}
