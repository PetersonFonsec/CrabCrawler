import { Directive, ElementRef, OnDestroy, afterNextRender, inject, input } from '@angular/core';
import { MotionService } from '../../core/motion.service';

/**
 * Conta de 0 até o valor quando o número aparece na tela.
 * O valor final já vem renderizado do servidor (SEO e leitores de tela).
 */
@Directive({ selector: '[appCountUp]' })
export class CountUpDirective implements OnDestroy {
  readonly appCountUp = input.required<number>();
  readonly decimals = input(0);
  readonly duration = input(1600);

  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly motion = inject(MotionService);
  private observer?: IntersectionObserver;
  private frame = 0;

  constructor() {
    afterNextRender(() => {
      if (!this.motion.motionEffects || !('IntersectionObserver' in window)) return;
      const node = this.el.nativeElement;
      node.textContent = this.format(0);

      this.observer = new IntersectionObserver(
        ([entry]) => {
          if (!entry.isIntersecting) return;
          this.observer?.disconnect();
          this.animate();
        },
        { threshold: 0.4 },
      );
      this.observer.observe(node);
    });
  }

  private animate(): void {
    const start = performance.now();
    const end = this.appCountUp();
    const step = (now: number) => {
      const t = Math.min(1, (now - start) / this.duration());
      const eased = 1 - Math.pow(1 - t, 4);
      this.el.nativeElement.textContent = this.format(end * eased);
      if (t < 1) this.frame = requestAnimationFrame(step);
    };
    this.frame = requestAnimationFrame(step);
  }

  private format(value: number): string {
    return value.toLocaleString('pt-BR', {
      minimumFractionDigits: this.decimals(),
      maximumFractionDigits: this.decimals(),
    });
  }

  ngOnDestroy(): void {
    this.observer?.disconnect();
    if (this.frame) cancelAnimationFrame(this.frame);
  }
}
