import { describe, expect, test } from 'vitest';
import { billableMinutes, chargeFor } from '../cost';
import { PAYG_RATE_CENTS } from '../pricing';

describe('billableMinutes', () => {
  test('zero duration bills nothing', () => {
    expect(billableMinutes(0)).toBe(0);
  });

  test('any audio at all bills a full minute', () => {
    expect(billableMinutes(1)).toBe(1);
    expect(billableMinutes(59)).toBe(1);
    expect(billableMinutes(60)).toBe(1);
  });

  test('partial minutes round up', () => {
    expect(billableMinutes(61)).toBe(2);
    expect(billableMinutes(1122)).toBe(19); // 18:42 from the mockup
  });
});

describe('chargeFor', () => {
  test('charges credit at the payg rate when no plan minutes remain', () => {
    expect(chargeFor(600, { minutesIncluded: 0, minutesUsed: 0 })).toEqual({
      minutesFromPlan: 0,
      centsFromCredit: 10 * PAYG_RATE_CENTS,
    });
  });

  test('consumes plan minutes before credit', () => {
    expect(chargeFor(600, { minutesIncluded: 300, minutesUsed: 0 })).toEqual({
      minutesFromPlan: 10,
      centsFromCredit: 0,
    });
  });

  test('splits across plan and credit when the plan runs out mid-file', () => {
    expect(chargeFor(600, { minutesIncluded: 300, minutesUsed: 296 })).toEqual({
      minutesFromPlan: 4,
      centsFromCredit: 6 * PAYG_RATE_CENTS,
    });
  });
});
