import { Injectable, PLATFORM_ID, inject, signal } from '@angular/core';
import { isPlatformBrowser } from '@angular/common';

/**
 * Centraliza as preferências de movimento do usuário.
 * Todos os efeitos (cursor, magnetismo, parallax, scroll horizontal)
 * consultam este serviço para respeitar `prefers-reduced-motion`
 * e para desligar interações de mouse em telas de toque.
 */
@Injectable({ providedIn: 'root' })
export class MotionService {
  readonly isBrowser = isPlatformBrowser(inject(PLATFORM_ID));
  readonly reducedMotion = signal(false);
  readonly finePointer = signal(false);

  constructor() {
    if (!this.isBrowser) return;

    const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
    const fine = window.matchMedia('(hover: hover) and (pointer: fine)');

    this.reducedMotion.set(reduced.matches);
    this.finePointer.set(fine.matches);

    reduced.addEventListener('change', (e) => this.reducedMotion.set(e.matches));
    fine.addEventListener('change', (e) => this.finePointer.set(e.matches));
  }

  /** Efeitos que dependem do mouse: só com ponteiro fino e sem redução de movimento. */
  get pointerEffects(): boolean {
    return this.isBrowser && this.finePointer() && !this.reducedMotion();
  }

  /** Efeitos ligados ao scroll ou animações contínuas. */
  get motionEffects(): boolean {
    return this.isBrowser && !this.reducedMotion();
  }
}

export const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
export const clamp = (v: number, min = 0, max = 1) => Math.min(max, Math.max(min, v));
