import { ChangeDetectionStrategy, Component, computed, signal } from '@angular/core';
import { DecimalPipe } from '@angular/common';
import { RevealDirective } from '../../../../shared/directives/reveal.directive';
import { MagneticDirective } from '../../../../shared/directives/magnetic.directive';
import { SplitWordsPipe } from '../../../../shared/pipes/split-words.pipe';

interface Place {
  name: string;
  city: string;
  price: string;
  scores: number[];
}

@Component({
  selector: 'app-compare',
  imports: [DecimalPipe, RevealDirective, MagneticDirective, SplitWordsPipe],
  templateUrl: './compare.component.html',
  styleUrl: './compare.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CompareComponent {
  readonly metrics = ['Segurança', 'Saúde', 'Educação', 'Mobilidade', 'Áreas verdes', 'Silêncio'];

  // Dados de demonstração para a interação; a versão real virá da API.
  readonly places: Place[] = [
    { name: 'Pinheiros', city: 'São Paulo', price: 'R$ 14.900/m²', scores: [7.8, 9.2, 8.1, 9.4, 6.1, 4.9] },
    { name: 'Moema', city: 'São Paulo', price: 'R$ 15.600/m²', scores: [8.6, 8.8, 8.7, 8.2, 8.9, 6.8] },
    { name: 'Botafogo', city: 'Rio de Janeiro', price: 'R$ 13.200/m²', scores: [6.9, 8.5, 7.9, 9.0, 7.4, 5.2] },
    { name: 'Savassi', city: 'Belo Horizonte', price: 'R$ 11.400/m²', scores: [7.4, 9.0, 8.3, 7.6, 5.8, 5.9] },
  ];

  readonly left = signal(0);
  readonly right = signal(1);

  readonly a = computed(() => this.places[this.left()]);
  readonly b = computed(() => this.places[this.right()]);

  readonly rows = computed(() =>
    this.metrics.map((metric, i) => {
      const va = this.a().scores[i];
      const vb = this.b().scores[i];
      return { metric, a: va, b: vb, winner: va === vb ? 0 : va > vb ? -1 : 1 };
    }),
  );

  readonly average = (p: Place) => p.scores.reduce((s, v) => s + v, 0) / p.scores.length;

  readonly verdict = computed(() => {
    const diff = this.average(this.a()) - this.average(this.b());
    if (Math.abs(diff) < 0.15) return 'Empate técnico. Vale olhar o que pesa mais para você.';
    const best = diff > 0 ? this.a() : this.b();
    return `${best.name} leva vantagem na média geral.`;
  });

  choose(side: 'left' | 'right', index: number): void {
    const other = side === 'left' ? this.right : this.left;
    const self = side === 'left' ? this.left : this.right;
    if (other() === index) other.set(self());
    self.set(index);
  }

  swap(): void {
    const l = this.left();
    this.left.set(this.right());
    this.right.set(l);
  }
}
