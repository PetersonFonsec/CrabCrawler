import { Component, Inject, PLATFORM_ID } from '@angular/core';

import { FooterComponent } from "./shared/components/layout/footer/footer.component";
import { HeaderComponent } from "./shared/components/layout/header/header.component";
import { HowWorksComponent } from './shared/components/layout/how-works/how-works.component';
import { ResumeExampleComponent } from './shared/components/layout/resume-example/resume-example.component';
import { LandingPageComponent } from "./pages/landing-page/landing-page.component";
import { FaqComponent } from './shared/components/layout/faq/faq.component';
import { isPlatformBrowser } from '@angular/common';
import Aos from 'aos';

@Component({
  selector: 'app-root',
  imports: [
    FooterComponent,
    HeaderComponent,
    HowWorksComponent,
    ResumeExampleComponent,
    LandingPageComponent,
    FaqComponent
  ],
  templateUrl: './app.component.html',
  styleUrl: './app.component.scss'
})
export class AppComponent {
  title = 'crab-crawler';

  constructor(@Inject(PLATFORM_ID) private platformId: Object) { }

  ngAfterViewInit() {
    if (isPlatformBrowser(this.platformId)) {
      Aos.init({
        duration: 3000,
        once: true,
        offset: 100,
        easing: 'ease-in-out',
      });
    }
  }
}
