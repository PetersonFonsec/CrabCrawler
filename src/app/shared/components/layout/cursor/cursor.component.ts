import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  NgZone,
  OnDestroy,
  afterNextRender,
  inject,
  viewChild,
} from '@angular/core';
import { DOCUMENT } from '@angular/common';
import { MotionService, lerp } from '../../../../core/motion.service';

/**
 * Cursor customizado: um ponto que segue o mouse exatamente e um anel
 * que vem atrás com inércia. Elementos podem pedir um estado com
 * `data-cursor="hover|view|drag|text"` e um rótulo com `data-cursor-label`.
 */
@Component({
  selector: 'app-cursor',
  templateUrl: './cursor.component.html',
  styleUrl: './cursor.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { 'aria-hidden': 'true' },
})
export class CursorComponent implements OnDestroy {
  private readonly zone = inject(NgZone);
  private readonly motion = inject(MotionService);
  private readonly document = inject(DOCUMENT);

  private readonly dot = viewChild.required<ElementRef<HTMLElement>>('dot');
  private readonly ring = viewChild.required<ElementRef<HTMLElement>>('ring');
  private readonly label = viewChild.required<ElementRef<HTMLElement>>('label');

  private frame = 0;
  private cleanup: Array<() => void> = [];

  constructor() {
    afterNextRender(() => {
      if (!this.motion.pointerEffects) return;
      this.zone.runOutsideAngular(() => this.bind());
    });
  }

  private bind(): void {
    const root = this.document.documentElement;
    const dot = this.dot().nativeElement;
    const ring = this.ring().nativeElement;
    const label = this.label().nativeElement;

    const mouse = { x: -100, y: -100 };
    const pos = { x: -100, y: -100 };
    let visible = false;

    root.classList.add('has-cursor');

    const onMove = (e: PointerEvent) => {
      if (e.pointerType !== 'mouse') return;
      mouse.x = e.clientX;
      mouse.y = e.clientY;
      dot.style.transform = `translate3d(${mouse.x}px, ${mouse.y}px, 0)`;
      if (!visible) {
        visible = true;
        pos.x = mouse.x;
        pos.y = mouse.y;
        root.classList.add('cursor-visible');
      }
    };

    const onOver = (e: Event) => {
      const target = (e.target as HTMLElement).closest<HTMLElement>(
        '[data-cursor], a, button, input, textarea, select, label, summary',
      );
      let state = '';
      let text = '';
      if (target) {
        state = target.dataset['cursor'] ?? (target.matches('input, textarea') ? 'text' : 'hover');
        text = target.dataset['cursorLabel'] ?? '';
      }
      ring.dataset['state'] = state;
      dot.dataset['state'] = state;
      label.textContent = text;
    };

    const onLeaveWindow = () => {
      visible = false;
      root.classList.remove('cursor-visible');
    };
    const onDown = () => ring.classList.add('is-down');
    const onUp = () => ring.classList.remove('is-down');

    const tick = () => {
      pos.x = lerp(pos.x, mouse.x, 0.16);
      pos.y = lerp(pos.y, mouse.y, 0.16);
      ring.style.transform = `translate3d(${pos.x}px, ${pos.y}px, 0)`;
      this.frame = requestAnimationFrame(tick);
    };
    this.frame = requestAnimationFrame(tick);

    const doc = this.document;
    doc.addEventListener('pointermove', onMove, { passive: true });
    doc.addEventListener('pointerover', onOver, { passive: true });
    doc.addEventListener('pointerdown', onDown);
    doc.addEventListener('pointerup', onUp);
    doc.documentElement.addEventListener('pointerleave', onLeaveWindow);

    this.cleanup.push(
      () => doc.removeEventListener('pointermove', onMove),
      () => doc.removeEventListener('pointerover', onOver),
      () => doc.removeEventListener('pointerdown', onDown),
      () => doc.removeEventListener('pointerup', onUp),
      () => doc.documentElement.removeEventListener('pointerleave', onLeaveWindow),
      () => root.classList.remove('has-cursor', 'cursor-visible'),
    );
  }

  ngOnDestroy(): void {
    if (this.frame) cancelAnimationFrame(this.frame);
    this.cleanup.forEach((fn) => fn());
  }
}
