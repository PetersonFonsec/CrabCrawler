import { ChangeDetectionStrategy, Component } from '@angular/core';
import { CountUpDirective } from '../../../../shared/directives/count-up.directive';
import { RevealDirective } from '../../../../shared/directives/reveal.directive';
import { TiltDirective } from '../../../../shared/directives/tilt.directive';
import { SplitWordsPipe } from '../../../../shared/pipes/split-words.pipe';

interface Indicator {
  name: string;
  score: number;
  text: string;
  source: string;
  icon: string;
}

@Component({
  selector: 'app-indicators',
  imports: [CountUpDirective, RevealDirective, TiltDirective, SplitWordsPipe],
  templateUrl: './indicators.component.html',
  styleUrl: './indicators.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class IndicatorsComponent {
  // Valores ilustrativos de um bairro de exemplo.
  readonly indicators: Indicator[] = [
    {
      name: 'Segurança',
      score: 8.2,
      text: 'Ocorrências por mil habitantes, iluminação e tendência dos últimos 12 meses.',
      source: 'SSP',
      icon: 'M12 2 4 5v6c0 5 3.4 9.4 8 11 4.6-1.6 8-6 8-11V5z',
    },
    {
      name: 'Saúde',
      score: 9.1,
      text: 'Hospitais, UBS e prontos-socorros a até 15 minutos.',
      source: 'DATASUS',
      icon: 'M10 3h4v7h7v4h-7v7h-4v-7H3v-4h7z',
    },
    {
      name: 'Educação',
      score: 7.6,
      text: 'Escolas e creches por perto, com nota do IDEB quando disponível.',
      source: 'INEP',
      icon: 'M12 3 1 9l11 6 9-4.9V17h2V9zM5 13.2v4L12 21l7-3.8v-4L12 17z',
    },
    {
      name: 'Mobilidade',
      score: 8.8,
      text: 'Distância até metrô, trem, corredores de ônibus e ciclovias.',
      source: 'OpenStreetMap',
      icon: 'M6 3h12a3 3 0 0 1 3 3v9a3 3 0 0 1-2 2.8V21h-3v-3H8v3H5v-3.2A3 3 0 0 1 3 15V6a3 3 0 0 1 3-3zm0 3v5h12V6zm1 7a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3zm10 0a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z',
    },
    {
      name: 'Áreas verdes',
      score: 6.4,
      text: 'Parques, praças e arborização num raio de 1 km.',
      source: 'Prefeituras',
      icon: 'M12 2a6 6 0 0 0-6 6c0 2.2 1.2 4.1 3 5.2V16H7v2h4v4h2v-4h4v-2h-2v-2.8A6 6 0 0 0 12 2z',
    },
    {
      name: 'Preço justo',
      score: 7.2,
      text: 'Valor do m² comparado à média do bairro e à valorização recente.',
      source: 'Portais',
      icon: 'M3 17 9 11l4 4 8-8v4h2V3h-8v2h4l-6 6-4-4-7 7z',
    },
  ];
}
