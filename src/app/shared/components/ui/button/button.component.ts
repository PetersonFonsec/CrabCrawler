import { Component, input } from '@angular/core';

@Component({
  selector: 'app-button',
  imports: [],
  templateUrl: './button.component.html',
  styleUrl: './button.component.scss'
})
export class ButtonComponent {
  readonly color = input("primary");
  readonly size = input("medium");
  readonly id = input("", { transform: (value: string /*T:VAE*/) => `${value}-button` });
  readonly disabled = input(false);
  readonly type = input("button");
}
