import { Routes } from '@angular/router';
import { LandingPageComponent } from './pages/landing-page/landing-page.component';

export const routes: Routes = [
  {
    path: '',
    redirectTo: '/landing-page',
    pathMatch: 'full'
  },
  {
    path: 'landing-page',
    component: LandingPageComponent,
    title: 'Use Venus Store',
  },
];
