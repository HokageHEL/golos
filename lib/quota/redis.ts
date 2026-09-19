import { Redis } from '@upstash/redis';
let cached: Redis | null = null;
export function redis(): Redis {
  if (!cached) cached = Redis.fromEnv();
  return cached;
}
export interface QuotaStore {
  reserveTrial(key: string, budget: string, ttl: number, cost: number, ceiling: number):
    Promise<'granted' | 'already_used' | 'budget_exhausted'>;
}
export function redisStore(client: Redis = redis()): QuotaStore {
  return {
    async reserveTrial(key, budget, ttl, cost, ceiling) {
      const result = await client.eval<number[], number>(`
        if redis.call('EXISTS', KEYS[1]) == 1 then return 1 end
        local spent = tonumber(redis.call('GET', KEYS[2]) or '0')
        if spent + tonumber(ARGV[2]) > tonumber(ARGV[3]) then return 2 end
        redis.call('SET', KEYS[1], '1', 'EX', ARGV[1])
        redis.call('INCRBY', KEYS[2], ARGV[2])
        redis.call('EXPIRE', KEYS[2], 172800)
        return 0
      `, [key, budget], [ttl, cost, ceiling]);
      return result === 0 ? 'granted' : result === 1 ? 'already_used' : 'budget_exhausted';
    },
  };
}
