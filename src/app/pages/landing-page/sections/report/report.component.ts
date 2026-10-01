import { ChangeDetectionStrategy, Component } from '@angular/core';
import { RevealDirective } from '../../../../shared/directives/reveal.directive';
import { SplitWordsPipe } from '../../../../shared/pipes/split-words.pipe';

@Component({
  selector: 'app-report',
  imports: [RevealDirective, SplitWordsPipe],
  templateUrl: './report.component.html',
  styleUrl: './report.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ReportComponent {
  readonly items = [
    { title: 'Mapa interativo', text: 'Tudo o que existe num raio de 1 km: escolas, hospitais, mercados, estações e pontos de atenção.' },
    { title: 'Indicadores explicados', text: 'Cada nota vem com o porquê, a fonte e a data de atualização. Nada de caixa-preta.' },
    { title: 'Histórico de preço', text: 'Valor do m² do bairro nos últimos anos para saber se o preço pedido faz sentido.' },
    { title: 'PDF para levar', text: 'Um relatório bonito para mandar para quem vai morar com você, ou para o corretor.' },
  ];
}
