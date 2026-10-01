import { Directive, ElementRef, NgZone, OnDestroy, afterNextRender, inject, input } from '@angular/core';
import { MotionService } from '../../core/motion.service';

/**
 * Inclina o card em 3D seguindo o cursor e expõe `--mx`/`--my`
 * (posição do mouse em %) para efeitos de brilho no CSS.
 */
@Directive({ selector: '[appTilt]' })
export class TiltDirective implements OnDestroy {
  /** Inclinação máxima em graus. */
  readonly appTilt = input<number | ''>(8);

  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly zone = inject(NgZone);
  private readonly motion = inject(MotionService);
  private cleanup: Array<() => void> = [];
  private frame = 0;

  constructor() {
    afterNextRender(() => {
      if (!this.motion.pointerEffects) return;
      this.zone.runOutsideAngular(() => this.bind());
    });
  }

  private bind(): void {
    const node = this.el.nativeElement;
    const max = typeof this.appTilt() === 'number' ? (this.appTilt() as number) : 8;

    const onMove = (e: PointerEvent) => {
      cancelAnimationFrame(this.frame);
      this.frame = requestAnimationFrame(() => {
        const rect = node.getBoundingClientRect();
        const px = (e.clientX - rect.left) / rect.width;
        const py = (e.clientY - rect.top) / rect.height;
        node.style.setProperty('--mx', `${px * 100}%`);
        node.style.setProperty('--my', `${py * 100}%`);
        node.style.transform = `perspective(900px) rotateX(${(0.5 - py) * max}deg) rotateY(${(px - 0.5) * max}deg)`;
      });
    };

    const onLeave = () => {
      cancelAnimationFrame(this.frame);
      node.style.transform = '';
    };

    node.addEventListener('pointermove', onMove);
    node.addEventListener('pointerleave', onLeave);
    this.cleanup.push(
      () => node.removeEventListener('pointermove', onMove),
      () => node.removeEventListener('pointerleave', onLeave),
    );
  }

  ngOnDestroy(): void {
    if (this.frame) cancelAnimationFrame(this.frame);
    this.cleanup.forEach((fn) => fn());
  }
}
