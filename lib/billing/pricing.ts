/**
 * All prices live here. Regional pricing (post-v1) adds a dimension to this
 * file and nothing else — no call site does its own arithmetic.
 */
export const PAYG_RATE_CENTS = 5; // $0.05 per minute

export const SUBSCRIPTION = {
  priceCents: 900, // $9.00 per month
  minutesIncluded: 300,
} as const;

export const TOPUP_OPTIONS_CENTS = [1000, 2500, 5000] as const;

/** Free anonymous trial: one file, capped at this many seconds. */
export const ANON_TRIAL_SECONDS = 600;

/** Stop serving anonymous trials once this much has been spent today. */
export const ANON_DAILY_BUDGET_CENTS = 500;
