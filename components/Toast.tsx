'use client';

import { useEffect } from 'react';

const TOAST_DURATION_MS = 3200;

export default function Toast({
  message,
  onDismiss,
}: {
  message: string;
  onDismiss: () => void;
}) {
  useEffect(() => {
    const timer = setTimeout(onDismiss, TOAST_DURATION_MS);
    return () => clearTimeout(timer);
  }, [onDismiss]);

  return (
    <div
      role="status"
      className="toast"
      style={{
        transform: 'translateX(-50%)',
        animation: `golos-toast ${TOAST_DURATION_MS}ms ease forwards`,
      }}
    >
      {message}
    </div>
  );
}
