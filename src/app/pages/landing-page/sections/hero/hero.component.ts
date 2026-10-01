import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  NgZone,
  OnDestroy,
  afterNextRender,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MotionService, lerp } from '../../../../core/motion.service';
import { MagneticDirective } from '../../../../shared/directives/magnetic.directive';
import { SplitWordsPipe } from '../../../../shared/pipes/split-words.pipe';
import { MAP_HEIGHT, MAP_PINS, MAP_WIDTH, buildCityMap, scoreColor } from '../../data/city-map';

/**
 * Hero com a assinatura visual do site: um mapa da cidade e uma "lupa"
 * que segue o cursor revelando a camada de dados (nota de cada quadra).
 * Em telas de toque a lupa passeia sozinha; com movimento reduzido fica parada.
 */
@Component({
  selector: 'app-hero',
  imports: [FormsModule, MagneticDirective, SplitWordsPipe],
  templateUrl: './hero.component.html',
  styleUrl: './hero.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HeroComponent implements OnDestroy {
  private readonly zone = inject(NgZone);
  private readonly motion = inject(MotionService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly coords = viewChild.required<ElementRef<HTMLElement>>('coords');

  readonly mapWidth = MAP_WIDTH;
  readonly mapHeight = MAP_HEIGHT;
  readonly blocks = buildCityMap().map((b) => ({ ...b, color: scoreColor(b.score) }));
  readonly pins = MAP_PINS;
  readonly suggestions = ['Pinheiros, São Paulo', 'Botafogo, Rio de Janeiro', 'Savassi, Belo Horizonte'];

  readonly query = signal('');
  readonly status = signal('');

  private frame = 0;
  private visible = true;
  private cleanup: Array<() => void> = [];

  constructor() {
    afterNextRender(() => this.zone.runOutsideAngular(() => this.bind()));
  }

  pick(place: string): void {
    this.query.set(place);
    this.status.set('');
  }

  search(event: Event): void {
    event.preventDefault();
    const q = this.query().trim();
    this.status.set(
      q
        ? `Investigando "${q}"… a busca real chega quando a API estiver conectada.`
        : 'Digite um endereço, bairro ou CEP para começar.',
    );
  }

  private bind(): void {
    const host = this.host.nativeElement;
    const coords = this.coords().nativeElement;

    const target = { x: 0, y: 0 };
    const lens = { x: 0, y: 0 };
    const tilt = { x: 0, y: 0, tx: 0, ty: 0 };
    let pointerActive = false;
    let start = performance.now();

    // No celular o mapa ocupa a faixa de cima do hero; no desktop, o lado direito.
    const wide = () => host.clientWidth >= 1024;
    const place = () => {
      const rect = host.getBoundingClientRect();
      target.x = lens.x = rect.width * (wide() ? 0.72 : 0.62);
      target.y = lens.y = wide() ? rect.height * 0.46 : window.innerHeight * 0.23;
    };
    place();
    this.apply(host, coords, lens.x, lens.y, 0, 0);

    if (!this.motion.motionEffects) return;

    const onMove = (e: PointerEvent) => {
      if (e.pointerType !== 'mouse') return;
      const rect = host.getBoundingClientRect();
      pointerActive = true;
      target.x = e.clientX - rect.left;
      target.y = e.clientY - rect.top;
      tilt.tx = (e.clientX / rect.width - 0.5) * 2;
      tilt.ty = (e.clientY / rect.height - 0.5) * 2;
    };
    const onLeave = () => {
      pointerActive = false;
      start = performance.now();
    };

    const tick = (now: number) => {
      const rect = host.getBoundingClientRect();
      if (!pointerActive) {
        // Passeio automático (curva de Lissajous) quando não há mouse.
        const t = (now - start) / 1000;
        if (wide()) {
          target.x = rect.width * (0.62 + Math.sin(t * 0.45) * 0.22);
          target.y = rect.height * (0.5 + Math.sin(t * 0.7) * 0.18);
        } else {
          target.x = rect.width * (0.5 + Math.sin(t * 0.5) * 0.28);
          target.y = window.innerHeight * (0.23 + Math.sin(t * 0.8) * 0.04);
        }
      }
      lens.x = lerp(lens.x, target.x, 0.12);
      lens.y = lerp(lens.y, target.y, 0.12);
      tilt.x = lerp(tilt.x, tilt.tx, 0.06);
      tilt.y = lerp(tilt.y, tilt.ty, 0.06);

      const scroll = Math.min(1, Math.max(0, -rect.top / rect.height));
      host.style.setProperty('--scroll', scroll.toFixed(4));
      this.apply(host, coords, lens.x, lens.y, tilt.x, tilt.y);

      this.frame = this.visible ? requestAnimationFrame(tick) : 0;
    };

    const observer = new IntersectionObserver(([entry]) => {
      this.visible = entry.isIntersecting;
      if (this.visible && !this.frame) this.frame = requestAnimationFrame(tick);
    });
    observer.observe(host);

    host.addEventListener('pointermove', onMove, { passive: true });
    host.addEventListener('pointerleave', onLeave);
    window.addEventListener('resize', place);

    this.cleanup.push(
      () => observer.disconnect(),
      () => host.removeEventListener('pointermove', onMove),
      () => host.removeEventListener('pointerleave', onLeave),
      () => window.removeEventListener('resize', place),
    );
  }

  private apply(host: HTMLElement, coords: HTMLElement, x: number, y: number, tx: number, ty: number): void {
    host.style.setProperty('--lx', `${x.toFixed(1)}px`);
    host.style.setProperty('--ly', `${y.toFixed(1)}px`);
    host.style.setProperty('--tx', tx.toFixed(3));
    host.style.setProperty('--ty', ty.toFixed(3));

    // Coordenadas fictícias em torno de São Paulo, só para dar vida ao leitor da lupa.
    const lat = -23.52 - (y / host.clientHeight) * 0.09;
    const lng = -46.72 + (x / host.clientWidth) * 0.14;
    coords.textContent = `${lat.toFixed(4)}, ${lng.toFixed(4)}`;
  }

  ngOnDestroy(): void {
    if (this.frame) cancelAnimationFrame(this.frame);
    this.cleanup.forEach((fn) => fn());
  }
}
