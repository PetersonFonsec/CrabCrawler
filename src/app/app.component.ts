import { Component } from '@angular/core';
import { RouterOutlet } from '@angular/router';

import { CursorComponent } from './shared/components/layout/cursor/cursor.component';
import { FooterComponent } from './shared/components/layout/footer/footer.component';
import { HeaderComponent } from './shared/components/layout/header/header.component';

@Component({
  selector: 'app-root',
  imports: [RouterOutlet, CursorComponent, FooterComponent, HeaderComponent],
  templateUrl: './app.component.html',
  styleUrl: './app.component.scss',
})
export class AppComponent {
  title = 'crab-crawler';
}
