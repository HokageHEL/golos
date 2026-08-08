import { Redis } from '@upstash/redis';

let cached: Redis | null = null;

export function redis(): Redis {
  if (!cached) cached = Redis.fromEnv();
  return cached;
}

/** Minimal surface the quota logic needs — keeps it testable without a server. */
export interface QuotaStore {
  claimOnce(key: string, ttlSeconds: number): Promise<boolean>;
  incrementBy(key: string, amount: number, ttlSeconds: number): Promise<number>;
  get(key: string): Promise<number>;
}

export function redisStore(client: Redis = redis()): QuotaStore {
  return {
    async claimOnce(key, ttlSeconds) {
      const result = await client.set(key, 1, { nx: true, ex: ttlSeconds });
      return result === 'OK';
    },
    async incrementBy(key, amount, ttlSeconds) {
      const value = await client.incrby(key, amount);
      if (value === amount) await client.expire(key, ttlSeconds);
      return value;
    },
    async get(key) {
      return (await client.get<number>(key)) ?? 0;
    },
  };
}
