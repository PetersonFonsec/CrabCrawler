import { Directive, ElementRef, NgZone, OnDestroy, afterNextRender, inject, input } from '@angular/core';
import { MotionService, lerp } from '../../core/motion.service';

/**
 * Faz o elemento ser "atraído" pelo cursor quando ele se aproxima.
 * O conteúdo interno com `[data-magnetic-inner]` se move um pouco mais,
 * criando profundidade. Desligado em toque e com movimento reduzido.
 */
@Directive({ selector: '[appMagnetic]' })
export class MagneticDirective implements OnDestroy {
  /** Intensidade da atração (0 a 1). */
  readonly appMagnetic = input<number | ''>(0.35);

  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly zone = inject(NgZone);
  private readonly motion = inject(MotionService);

  private frame = 0;
  private target = { x: 0, y: 0 };
  private current = { x: 0, y: 0 };
  private cleanup: Array<() => void> = [];

  constructor() {
    afterNextRender(() => {
      if (!this.motion.pointerEffects) return;
      this.zone.runOutsideAngular(() => this.bind());
    });
  }

  private get strength(): number {
    const value = this.appMagnetic();
    return typeof value === 'number' ? value : 0.35;
  }

  private bind(): void {
    const node = this.el.nativeElement;
    const inner = node.querySelector<HTMLElement>('[data-magnetic-inner]');

    const onMove = (e: PointerEvent) => {
      const rect = node.getBoundingClientRect();
      this.target.x = (e.clientX - (rect.left + rect.width / 2)) * this.strength;
      this.target.y = (e.clientY - (rect.top + rect.height / 2)) * this.strength;
      this.start();
    };

    const onLeave = () => {
      this.target.x = 0;
      this.target.y = 0;
      this.start();
    };

    const tick = () => {
      this.current.x = lerp(this.current.x, this.target.x, 0.18);
      this.current.y = lerp(this.current.y, this.target.y, 0.18);
      node.style.transform = `translate3d(${this.current.x}px, ${this.current.y}px, 0)`;
      if (inner) {
        inner.style.transform = `translate3d(${this.current.x * 0.4}px, ${this.current.y * 0.4}px, 0)`;
      }

      const settled =
        Math.abs(this.current.x - this.target.x) < 0.1 && Math.abs(this.current.y - this.target.y) < 0.1;
      this.frame = settled ? 0 : requestAnimationFrame(tick);
    };

    this.start = () => {
      if (!this.frame) this.frame = requestAnimationFrame(tick);
    };

    node.addEventListener('pointermove', onMove);
    node.addEventListener('pointerleave', onLeave);
    this.cleanup.push(
      () => node.removeEventListener('pointermove', onMove),
      () => node.removeEventListener('pointerleave', onLeave),
    );
  }

  private start: () => void = () => {};

  ngOnDestroy(): void {
    if (this.frame) cancelAnimationFrame(this.frame);
    this.cleanup.forEach((fn) => fn());
  }
}
