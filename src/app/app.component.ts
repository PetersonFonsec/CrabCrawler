import { Component } from '@angular/core';
import { FooterComponent } from "./shared/components/layout/footer/footer.component";
import { HeaderComponent } from "./shared/components/layout/header/header.component";
import { FaqComponent } from './shared/components/layout/faq/faq.component';
import { HowWorksComponent } from './shared/components/layout/how-works/how-works.component';
import { ResumeExampleComponent } from './shared/components/layout/resume-example/resume-example.component';
import { LandingPageComponent } from "./pages/landing-page/landing-page.component";

@Component({
  selector: 'app-root',
  imports: [FooterComponent, HeaderComponent, FaqComponent, HowWorksComponent, ResumeExampleComponent, LandingPageComponent],
  templateUrl: './app.component.html',
  styleUrl: './app.component.scss'
})
export class AppComponent {
  title = 'crab-crawler';
}
