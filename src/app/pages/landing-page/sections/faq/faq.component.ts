import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { RevealDirective } from '../../../../shared/directives/reveal.directive';

@Component({
  selector: 'app-faq',
  imports: [RevealDirective],
  templateUrl: './faq.component.html',
  styleUrl: './faq.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FaqComponent {
  readonly open = signal<number | null>(0);

  readonly items = [
    {
      q: 'De onde vêm os dados?',
      a: 'De bases públicas como IBGE, DATASUS, INEP, secretarias de segurança e OpenStreetMap, além de anúncios públicos de imóveis. Cada indicador mostra a fonte e a data.',
    },
    {
      q: 'Funciona em qualquer cidade?',
      a: 'Começamos pelas capitais e regiões metropolitanas. Onde algum dado não existe, o relatório avisa em vez de inventar um número.',
    },
    {
      q: 'Posso comparar mais de um imóvel?',
      a: 'Sim. Você pode salvar imóveis e comparar bairros lado a lado, indicador por indicador.',
    },
    {
      q: 'Quanto custa?',
      a: 'A busca e as notas principais são gratuitas. O relatório completo em PDF será um plano pago, ainda em definição.',
    },
  ];

  toggle(index: number): void {
    this.open.update((current) => (current === index ? null : index));
  }
}
