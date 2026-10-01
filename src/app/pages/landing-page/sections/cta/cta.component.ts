import { ChangeDetectionStrategy, Component } from '@angular/core';
import { MagneticDirective } from '../../../../shared/directives/magnetic.directive';
import { RevealDirective } from '../../../../shared/directives/reveal.directive';
import { SplitWordsPipe } from '../../../../shared/pipes/split-words.pipe';

@Component({
  selector: 'app-cta',
  imports: [MagneticDirective, RevealDirective, SplitWordsPipe],
  templateUrl: './cta.component.html',
  styleUrl: './cta.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class CtaComponent {}
