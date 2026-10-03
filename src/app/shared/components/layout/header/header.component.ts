import {
  ChangeDetectionStrategy,
  Component,
  NgZone,
  OnDestroy,
  afterNextRender,
  inject,
  signal,
} from '@angular/core';
import { DOCUMENT } from '@angular/common';
import { MagneticDirective } from '../../../directives/magnetic.directive';

interface NavLink {
  label: string;
  href: string;
}

@Component({
  selector: 'app-header',
  imports: [MagneticDirective],
  templateUrl: './header.component.html',
  styleUrl: './header.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[class.is-hidden]': 'hidden() && !menuOpen()',
    '[class.is-solid]': 'solid()',
    '[class.menu-open]': 'menuOpen()',
    '(document:keydown.escape)': 'closeMenu()',
  },
})
export class HeaderComponent implements OnDestroy {
  private readonly zone = inject(NgZone);
  private readonly document = inject(DOCUMENT);

  readonly links: NavLink[] = [
    { label: 'Como funciona', href: '/#como-funciona' },
    { label: 'Indicadores', href: '/#indicadores' },
    { label: 'Comparar', href: '/#comparar' },
    { label: 'Fontes', href: '/#fontes' },
    { label: 'Dúvidas', href: '/#duvidas' },
    { label: 'Analisar imóvel', href: '/analisar' },
  ];

  readonly hidden = signal(false);
  readonly solid = signal(false);
  readonly menuOpen = signal(false);

  private removeScroll?: () => void;

  constructor() {
    afterNextRender(() => {
      let last = window.scrollY;
      let ticking = false;

      const update = () => {
        const y = window.scrollY;
        const goingDown = y > last && y > 160;
        const solid = y > 24;
        ticking = false;
        last = y;
        if (goingDown !== this.hidden() || solid !== this.solid()) {
          this.zone.run(() => {
            this.hidden.set(goingDown);
            this.solid.set(solid);
          });
        }
      };

      const onScroll = () => {
        if (!ticking) {
          ticking = true;
          requestAnimationFrame(update);
        }
      };

      this.zone.runOutsideAngular(() => window.addEventListener('scroll', onScroll, { passive: true }));
      this.removeScroll = () => window.removeEventListener('scroll', onScroll);
      update();
    });
  }

  toggleMenu(): void {
    this.menuOpen.update((open) => !open);
    this.document.body.style.overflow = this.menuOpen() ? 'hidden' : '';
  }

  closeMenu(): void {
    if (!this.menuOpen()) return;
    this.menuOpen.set(false);
    this.document.body.style.overflow = '';
  }

  ngOnDestroy(): void {
    this.removeScroll?.();
  }
}
