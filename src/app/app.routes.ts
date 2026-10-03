import { Routes } from '@angular/router';
import { LandingPageComponent } from './pages/landing-page/landing-page.component';
import { AnalyzePropertyComponent } from './pages/analyze-property/analyze-property.component';

export const routes: Routes = [
  { path: '', component: LandingPageComponent, title: 'CrabCrawler | Saiba onde você está se metendo' },
  { path: 'analisar', component: AnalyzePropertyComponent, title: 'Analisar um imóvel | CrabCrawler' },
  { path: '**', redirectTo: '' },
];
