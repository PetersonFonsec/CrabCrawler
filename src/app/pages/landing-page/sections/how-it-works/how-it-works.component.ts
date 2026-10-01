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
import { MotionService, clamp } from '../../../../core/motion.service';
import { RevealDirective } from '../../../../shared/directives/reveal.directive';
import { SplitWordsPipe } from '../../../../shared/pipes/split-words.pipe';

interface Step {
  title: string;
  text: string;
  art: 'search' | 'crawl' | 'cross' | 'decide';
}

/**
 * Seção fixada na tela em que o scroll vertical vira movimento horizontal.
 * Só liga em telas grandes e sem movimento reduzido; senão vira uma lista vertical.
 */
@Component({
  selector: 'app-how-it-works',
  imports: [RevealDirective, SplitWordsPipe],
  templateUrl: './how-it-works.component.html',
  styleUrl: './how-it-works.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[class.is-pinned]': 'pinned()' },
})
export class HowItWorksComponent implements OnDestroy {
  private readonly zone = inject(NgZone);
  private readonly motion = inject(MotionService);
  private readonly section = viewChild.required<ElementRef<HTMLElement>>('section');
  private readonly track = viewChild.required<ElementRef<HTMLElement>>('track');
  private readonly bar = viewChild.required<ElementRef<HTMLElement>>('bar');

  readonly steps: Step[] = [
    {
      title: 'Busque',
      text: 'Digite um endereço, bairro ou cole o link de um anúncio. A gente entende o que você procura.',
      art: 'search',
    },
    {
      title: 'Rastreamos',
      text: 'Nosso caranguejo vasculha bases públicas e portais e junta tudo o que existe sobre aquele pedaço da cidade.',
      art: 'crawl',
    },
    {
      title: 'Cruzamos',
      text: 'Segurança, saúde, escolas, transporte, ruído e preço viram indicadores simples, de 0 a 10.',
      art: 'cross',
    },
    {
      title: 'Decida',
      text: 'Compare imóveis lado a lado e receba um relatório para decidir com calma antes de assinar.',
      art: 'decide',
    },
  ];

  readonly pinned = signal(false);
  readonly active = signal(0);

  private cleanup: Array<() => void> = [];

  constructor() {
    afterNextRender(() => this.zone.runOutsideAngular(() => this.bind()));
  }

  private bind(): void {
    const section = this.section().nativeElement;
    const track = this.track().nativeElement;
    const bar = this.bar().nativeElement;
    const query = window.matchMedia('(min-width: 1024px)');
    let distance = 0;
    let ticking = false;

    const measure = () => {
      const shouldPin = query.matches && this.motion.motionEffects;
      if (shouldPin !== this.pinned()) this.zone.run(() => this.pinned.set(shouldPin));
      if (!shouldPin) {
        section.style.height = '';
        track.style.transform = '';
        return;
      }
      distance = track.scrollWidth - window.innerWidth;
      section.style.height = `${distance + window.innerHeight}px`;
      update();
    };

    const update = () => {
      ticking = false;
      if (!this.pinned()) return;
      const rect = section.getBoundingClientRect();
      const progress = clamp(-rect.top / (rect.height - window.innerHeight));
      track.style.transform = `translate3d(${-progress * distance}px, 0, 0)`;
      bar.style.transform = `scaleX(${progress})`;
      const step = Math.min(this.steps.length - 1, Math.floor(progress * this.steps.length));
      if (step !== this.active()) this.zone.run(() => this.active.set(step));
    };

    const onScroll = () => {
      if (!ticking) {
        ticking = true;
        requestAnimationFrame(update);
      }
    };

    // Espera as fontes para medir a largura real da faixa.
    document.fonts?.ready.then(measure);
    measure();

    window.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('resize', measure);
    query.addEventListener('change', measure);
    this.cleanup.push(
      () => window.removeEventListener('scroll', onScroll),
      () => window.removeEventListener('resize', measure),
      () => query.removeEventListener('change', measure),
    );
  }

  ngOnDestroy(): void {
    this.cleanup.forEach((fn) => fn());
  }
}
