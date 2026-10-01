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
import { MotionService, lerp } from '../../../../core/motion.service';
import { RevealDirective } from '../../../../shared/directives/reveal.directive';
import { SplitWordsPipe } from '../../../../shared/pipes/split-words.pipe';

interface Source {
  name: string;
  topic: string;
  text: string;
  color: string;
}

/**
 * Lista de fontes de dados. No hover, um cartão flutuante segue o cursor
 * com o detalhe da fonte (o mesmo texto já está na linha, para leitores de tela e toque).
 */
@Component({
  selector: 'app-sources',
  imports: [RevealDirective, SplitWordsPipe],
  templateUrl: './sources.component.html',
  styleUrl: './sources.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SourcesComponent implements OnDestroy {
  private readonly zone = inject(NgZone);
  private readonly motion = inject(MotionService);
  private readonly list = viewChild.required<ElementRef<HTMLElement>>('list');
  private readonly preview = viewChild.required<ElementRef<HTMLElement>>('preview');

  readonly sources: Source[] = [
    { name: 'IBGE', topic: 'Renda e população', text: 'Censo 2022: densidade, renda média e perfil dos moradores por setor censitário.', color: '#3a86c8' },
    { name: 'SSP', topic: 'Segurança', text: 'Boletins de ocorrência públicos agregados por região e tipo de ocorrência.', color: '#fa2f3a' },
    { name: 'DATASUS', topic: 'Saúde', text: 'Cadastro de hospitais, UBS e pronto-atendimentos com capacidade e especialidades.', color: '#06d6a0' },
    { name: 'INEP', topic: 'Educação', text: 'Escolas, creches e resultados do IDEB e do Censo Escolar.', color: '#fff288' },
    { name: 'OpenStreetMap', topic: 'Mobilidade e lazer', text: 'Ruas, estações, ciclovias, parques e comércio mapeados pela comunidade.', color: '#b892ff' },
    { name: 'Portais', topic: 'Preço', text: 'Anúncios públicos de imóveis para estimar o m² e a valorização do bairro.', color: '#ff9f43' },
  ];

  readonly current = signal(0);
  readonly showing = signal(false);

  private frame = 0;
  private cleanup: Array<() => void> = [];

  constructor() {
    afterNextRender(() => {
      if (!this.motion.pointerEffects) return;
      this.zone.runOutsideAngular(() => this.bind());
    });
  }

  enter(index: number): void {
    this.current.set(index);
    this.showing.set(true);
  }

  leave(): void {
    this.showing.set(false);
  }

  private bind(): void {
    const list = this.list().nativeElement;
    const preview = this.preview().nativeElement;
    const target = { x: 0, y: 0 };
    const pos = { x: 0, y: 0 };
    let last = 0;

    const onMove = (e: PointerEvent) => {
      const rect = list.getBoundingClientRect();
      target.x = e.clientX - rect.left;
      target.y = e.clientY - rect.top;
      if (!this.frame) this.frame = requestAnimationFrame(tick);
    };

    const tick = () => {
      const dx = target.x - pos.x;
      pos.x = lerp(pos.x, target.x, 0.14);
      pos.y = lerp(pos.y, target.y, 0.14);
      const rot = Math.max(-12, Math.min(12, dx * 0.05));
      last = lerp(last, rot, 0.2);
      preview.style.transform = `translate3d(${pos.x}px, ${pos.y}px, 0) rotate(${last}deg)`;
      const settled = Math.abs(target.x - pos.x) < 0.2 && Math.abs(target.y - pos.y) < 0.2;
      this.frame = settled ? 0 : requestAnimationFrame(tick);
    };

    list.addEventListener('pointermove', onMove, { passive: true });
    this.cleanup.push(() => list.removeEventListener('pointermove', onMove));
  }

  ngOnDestroy(): void {
    if (this.frame) cancelAnimationFrame(this.frame);
    this.cleanup.forEach((fn) => fn());
  }
}
