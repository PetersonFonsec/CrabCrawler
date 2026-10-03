import { InjectionToken } from '@angular/core';

/**
 * Endereço da API do backend (Rust). Em desenvolvimento é o `crabcrawler
 * serve` local; em produção pode ser definido antes do bundle com
 * `window.CRAB_API_URL`.
 */
export const API_BASE_URL = new InjectionToken<string>('API_BASE_URL', {
  providedIn: 'root',
  factory: () => {
    const fromWindow = (globalThis as { CRAB_API_URL?: string }).CRAB_API_URL;
    return (fromWindow ?? 'http://localhost:3000').replace(/\/$/, '');
  },
});
