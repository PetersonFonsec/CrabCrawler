import { Directive, ElementRef, OnDestroy, afterNextRender, inject, input } from '@angular/core';
import { MotionService } from '../../core/motion.service';

/**
 * Revela o elemento quando ele entra na viewport.
 * Os estados visuais ficam em `styles.scss` (`.reveal`), assim o
 * conteúdo continua visível quando não há JavaScript.
 *
 * Uso: `<h2 appReveal="mask" [revealDelay]="120">`
 */
@Directive({
  selector: '[appReveal]',
  host: {
    class: 'reveal',
    '[attr.data-reveal]': 'appReveal() || "up"',
    '[style.--reveal-delay.ms]': 'revealDelay()',
  },
})
export class RevealDirective implements OnDestroy {
  readonly appReveal = input<'up' | 'fade' | 'mask' | 'scale' | ''>('up');
  readonly revealDelay = input(0);

  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly motion = inject(MotionService);
  private observer?: IntersectionObserver;

  constructor() {
    afterNextRender(() => {
      const node = this.el.nativeElement;

      if (!('IntersectionObserver' in window) || this.motion.reducedMotion()) {
        node.classList.add('is-in');
        return;
      }

      this.observer = new IntersectionObserver(
        (entries) => {
          for (const entry of entries) {
            if (entry.isIntersecting) {
              node.classList.add('is-in');
              this.observer?.disconnect();
            }
          }
        },
        { rootMargin: '0px 0px -12% 0px', threshold: 0.05 },
      );
      this.observer.observe(node);
    });
  }

  ngOnDestroy(): void {
    this.observer?.disconnect();
  }
}
