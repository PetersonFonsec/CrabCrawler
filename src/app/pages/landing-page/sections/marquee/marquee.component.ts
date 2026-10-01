import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  NgZone,
  OnDestroy,
  afterNextRender,
  inject,
  viewChildren,
} from '@angular/core';
import { MotionService, lerp } from '../../../../core/motion.service';

/**
 * Faixas de texto infinitas que aceleram e inclinam conforme a
 * velocidade do scroll. Só animam quando estão visíveis.
 */
@Component({
  selector: 'app-marquee',
  templateUrl: './marquee.component.html',
  styleUrl: './marquee.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class MarqueeComponent implements OnDestroy {
  private readonly zone = inject(NgZone);
  private readonly motion = inject(MotionService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly tracks = viewChildren<ElementRef<HTMLElement>>('track');

  readonly rows = [
    ['Segurança', 'Escolas', 'Hospitais', 'Metrô', 'Parques', 'Ruído', 'Enchentes', 'Valorização'],
    ['IBGE', 'DATASUS', 'INEP', 'SSP', 'OpenStreetMap', 'Prefeituras', 'Censo 2022', 'Portais imobiliários'],
  ];

  private frame = 0;
  private cleanup: Array<() => void> = [];

  constructor() {
    afterNextRender(() => {
      if (!this.motion.motionEffects) return;
      this.zone.runOutsideAngular(() => this.bind());
    });
  }

  private bind(): void {
    const tracks = this.tracks().map((t) => t.nativeElement);
    const offsets = tracks.map(() => 0);
    let lastY = window.scrollY;
    let velocity = 0;
    let visible = false;

    const tick = () => {
      const y = window.scrollY;
      velocity = lerp(velocity, y - lastY, 0.1);
      lastY = y;

      tracks.forEach((track, i) => {
        const dir = i % 2 === 0 ? -1 : 1;
        const half = track.scrollWidth / 2;
        offsets[i] += dir * (0.6 + Math.abs(velocity) * 0.25);
        if (offsets[i] <= -half) offsets[i] += half;
        if (offsets[i] >= 0) offsets[i] -= half;
        track.style.transform = `translate3d(${offsets[i]}px, 0, 0) skewX(${Math.max(-12, Math.min(12, velocity * -0.4))}deg)`;
      });

      this.frame = visible ? requestAnimationFrame(tick) : 0;
    };

    // Começa deslocada para a faixa da direita não abrir vazia.
    tracks.forEach((track, i) => (offsets[i] = i % 2 === 0 ? 0 : -track.scrollWidth / 2));

    const observer = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting;
      if (visible && !this.frame) {
        lastY = window.scrollY;
        this.frame = requestAnimationFrame(tick);
      }
    });
    observer.observe(this.host.nativeElement);
    this.cleanup.push(() => observer.disconnect());
  }

  ngOnDestroy(): void {
    if (this.frame) cancelAnimationFrame(this.frame);
    this.cleanup.forEach((fn) => fn());
  }
}
