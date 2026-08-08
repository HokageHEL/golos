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
    <>
      <div
        role="status"
        style={{
          position: 'fixed',
          bottom: 32,
          left: '50%',
          transform: 'translateX(-50%)',
          borderRadius: 100,
          background: '#1C1B19',
          color: '#fff',
          fontSize: 14,
          padding: '12px 20px',
          boxShadow: '0 8px 24px rgba(0,0,0,0.16)',
          whiteSpace: 'nowrap',
          animation: `golos-toast ${TOAST_DURATION_MS}ms ease forwards`,
        }}
      >
        {message}
      </div>
      <style>{`
        @keyframes golos-toast {
          0% { transform: translate(-50%, 12px); opacity: 0; }
          10% { transform: translate(-50%, 0); opacity: 1; }
          90% { transform: translate(-50%, 0); opacity: 1; }
          100% { transform: translate(-50%, 0); opacity: 0; }
        }
      `}</style>
    </>
  );
}
