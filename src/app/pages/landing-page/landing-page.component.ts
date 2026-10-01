import { ChangeDetectionStrategy, Component } from '@angular/core';
import { HeroComponent } from './sections/hero/hero.component';
import { MarqueeComponent } from './sections/marquee/marquee.component';
import { HowItWorksComponent } from './sections/how-it-works/how-it-works.component';
import { IndicatorsComponent } from './sections/indicators/indicators.component';
import { CompareComponent } from './sections/compare/compare.component';
import { ReportComponent } from './sections/report/report.component';
import { SourcesComponent } from './sections/sources/sources.component';
import { FaqComponent } from './sections/faq/faq.component';
import { CtaComponent } from './sections/cta/cta.component';

@Component({
  selector: 'app-landing-page',
  imports: [
    HeroComponent,
    MarqueeComponent,
    HowItWorksComponent,
    IndicatorsComponent,
    CompareComponent,
    ReportComponent,
    SourcesComponent,
    FaqComponent,
    CtaComponent,
  ],
  templateUrl: './landing-page.component.html',
  styleUrl: './landing-page.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LandingPageComponent {}
