import { beforeEach, expect, test, vi } from 'vitest';
const mocks = vi.hoisted(() => ({ validate: vi.fn(), insert: vi.fn(), sync: vi.fn() }));
vi.mock('@polar-sh/sdk/webhooks.js', () => ({ validateEvent: mocks.validate, WebhookVerificationError: class extends Error {} }));
vi.mock('@/lib/supabase/server', () => ({ serviceClient: () => ({ from: () => ({ insert: mocks.insert }) }) }));
vi.mock('@/lib/billing/subscription', () => ({ syncSubscription: mocks.sync }));
import { POST } from '../route';
const order = { id:'order-test', paid:true, currency:'usd', netAmount:1000, billingReason:'purchase',
  productId:'configured-product', metadata:{}, customer:{externalId:'12345678-1234-1234-1234-123456789abc'} };
const request = () => new Request('https://golos.test/api/webhooks/polar', { method:'POST',body:'signed-payload' });
beforeEach(() => {
  vi.resetAllMocks(); vi.stubEnv('POLAR_WEBHOOK_SECRET','test-secret');
  vi.stubEnv('POLAR_TOPUP_PRODUCTS','1000:configured-product');
  mocks.insert.mockResolvedValue({error:null});
});
test('an unconfigured webhook fails closed', async () => {
  vi.stubEnv('POLAR_WEBHOOK_SECRET','');
  const log=vi.spyOn(console,'error').mockImplementation(()=>{});
  expect((await POST(request())).status).toBe(401); expect(mocks.insert).not.toHaveBeenCalled(); log.mockRestore();
});
test('credits only configured products and acknowledges duplicate orders', async () => {
  mocks.validate.mockReturnValue({type:'order.paid',data:{...order,productId:'unrelated'}});
  expect((await POST(request())).status).toBe(200); expect(mocks.insert).not.toHaveBeenCalled();
  mocks.validate.mockReturnValue({type:'order.paid',data:order});
  mocks.insert.mockResolvedValue({error:{code:'23505'}});
  expect((await POST(request())).status).toBe(200);
  expect(mocks.insert).toHaveBeenCalledWith(expect.objectContaining({delta_cents:1000,external_id:'polar:order:order-test'}));
});
test('credit failures request webhook redelivery', async () => {
  mocks.validate.mockReturnValue({type:'order.paid',data:order});
  mocks.insert.mockResolvedValue({error:{code:'database_down'}});
  const log=vi.spyOn(console,'error').mockImplementation(()=>{});
  expect((await POST(request())).status).toBe(500); log.mockRestore();
});
test('subscription notifications synchronize current provider state', async () => {
  mocks.validate.mockReturnValue({type:'subscription.updated',data:{id:'sub-1'}});
  expect((await POST(request())).status).toBe(200); expect(mocks.sync).toHaveBeenCalledWith('sub-1');
  expect(mocks.insert).not.toHaveBeenCalled();
});
