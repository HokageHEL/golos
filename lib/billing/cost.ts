import { PAYG_RATE_CENTS } from './pricing';

export function billableMinutes(durationSeconds: number): number {
  if (durationSeconds <= 0) return 0;
  return Math.ceil(durationSeconds / 60);
}

export type PlanState = { minutesIncluded: number; minutesUsed: number };

export type Charge = { minutesFromPlan: number; centsFromCredit: number };

export function chargeFor(durationSeconds: number, plan: PlanState): Charge {
  const minutes = billableMinutes(durationSeconds);
  const remaining = Math.max(0, plan.minutesIncluded - plan.minutesUsed);
  const minutesFromPlan = Math.min(minutes, remaining);

  return {
    minutesFromPlan,
    centsFromCredit: (minutes - minutesFromPlan) * PAYG_RATE_CENTS,
  };
}
