import { ChangeDetectionStrategy, Component } from '@angular/core';
import { RevealDirective } from '../../../directives/reveal.directive';

@Component({
  selector: 'app-footer',
  imports: [RevealDirective],
  templateUrl: './footer.component.html',
  styleUrl: './footer.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FooterComponent {
  readonly year = new Date().getFullYear();
  readonly letters = 'CrabCrawler'.split('');
  readonly links = [
    { label: 'Como funciona', href: '/#como-funciona' },
    { label: 'Indicadores', href: '/#indicadores' },
    { label: 'Comparar', href: '/#comparar' },
    { label: 'Fontes', href: '/#fontes' },
    { label: 'Dúvidas', href: '/#duvidas' },
  ];
}
