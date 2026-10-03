import { CurrencyPipe } from '@angular/common';
import { HttpErrorResponse } from '@angular/common/http';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { AbstractControl, FormBuilder, ReactiveFormsModule, ValidationErrors, Validators } from '@angular/forms';

import {
  FieldIssue,
  InvalidResponse,
  ManualCreated,
  ManualPropertyInput,
  PropertiesService,
  RegionResponse,
} from '../../core/properties.service';

export const PROPERTY_TYPES = [
  { value: 'apartment', label: 'Apartamento' },
  { value: 'house', label: 'Casa' },
  { value: 'condo_house', label: 'Casa em condomínio' },
  { value: 'penthouse', label: 'Cobertura' },
  { value: 'studio', label: 'Studio / kitnet' },
  { value: 'flat', label: 'Flat' },
  { value: 'land', label: 'Terreno' },
  { value: 'commercial', label: 'Comercial' },
];

const CEP = /^\d{5}-?\d{3}$/;

/** CEP, ou município + UF: sem um dos dois não há como localizar o imóvel. */
function locationAnchor(group: AbstractControl): ValidationErrors | null {
  const v = group.value as { postal_code?: string; municipality?: string; state?: string };
  const hasCep = !!v.postal_code?.trim();
  const hasCity = !!v.municipality?.trim() && !!v.state?.trim();
  return hasCep || hasCity ? null : { location: true };
}

/** Nome do campo da API → nome do controle no formulário. */
const FIELD_MAP: Record<string, string> = {
  sale_price: 'price',
  rent_price: 'price',
  price: 'price',
  postal_code: 'postal_code',
  location: 'postal_code',
  source_url: 'source_url',
  state: 'state',
  area_m2: 'area_m2',
};

type Phase = 'form' | 'saving' | 'done';

@Component({
  selector: 'app-analyze-property',
  imports: [ReactiveFormsModule, CurrencyPipe],
  templateUrl: './analyze-property.component.html',
  styleUrl: './analyze-property.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AnalyzePropertyComponent {
  private readonly fb = inject(FormBuilder);
  private readonly properties = inject(PropertiesService);

  readonly types = PROPERTY_TYPES;
  readonly phase = signal<Phase>('form');
  readonly showMore = signal(false);
  readonly serverErrors = signal<Record<string, string>>({});
  readonly generalError = signal<string | null>(null);
  readonly result = signal<ManualCreated | null>(null);
  readonly region = signal<RegionResponse | null>(null);
  readonly regionError = signal<string | null>(null);

  readonly form = this.fb.group(
    {
      transaction: this.fb.nonNullable.control<'sale' | 'rent'>('sale'),
      source_url: [''],
      price: this.fb.control<number | null>(null, [Validators.required, Validators.min(1)]),
      postal_code: ['', Validators.pattern(CEP)],
      street: [''],
      number: [''],
      property_type: [''],
      area_m2: this.fb.control<number | null>(null, Validators.min(1)),
      bedrooms: this.fb.control<number | null>(null, Validators.min(0)),
      parking_spaces: this.fb.control<number | null>(null, Validators.min(0)),
      // "Adicionar mais informações"
      complement: [''],
      neighborhood: [''],
      municipality: [''],
      state: ['', Validators.pattern(/^[A-Za-z]{2}$/)],
      bathrooms: this.fb.control<number | null>(null, Validators.min(0)),
      suites: this.fb.control<number | null>(null, Validators.min(0)),
      condominium_fee: this.fb.control<number | null>(null, Validators.min(0)),
      property_tax: this.fb.control<number | null>(null, Validators.min(0)),
      notes: [''],
    },
    { validators: locationAnchor },
  );

  toggleMore(): void {
    this.showMore.update((v) => !v);
  }

  /** Mensagem de erro de um campo (validação local ou resposta da API). */
  error(name: string): string | null {
    const server = this.serverErrors()[name];
    if (server) return server;
    const c = this.form.get(name);
    if (!c || !(c.touched || c.dirty) || !c.errors) return null;
    if (c.errors['required']) return 'Campo obrigatório.';
    if (c.errors['min']) return 'Valor inválido.';
    if (c.errors['pattern']) return name === 'postal_code' ? 'Use o formato 00000-000.' : 'Use a sigla com 2 letras.';
    return 'Valor inválido.';
  }

  locationMissing(): boolean {
    return !!this.form.errors?.['location'] && (this.form.touched || this.form.dirty);
  }

  submit(): void {
    this.serverErrors.set({});
    this.generalError.set(null);
    if (this.form.invalid) {
      this.form.markAllAsTouched();
      if (this.form.errors?.['location']) this.showMore.set(true);
      return;
    }
    this.phase.set('saving');
    this.properties.createManual(this.payload()).subscribe({
      next: (created) => {
        this.result.set(created);
        this.phase.set('done');
        this.loadRegion(created.property.id);
      },
      error: (err: HttpErrorResponse) => {
        this.phase.set('form');
        this.handleError(err);
      },
    });
  }

  reset(): void {
    this.form.reset({ transaction: 'sale' });
    this.result.set(null);
    this.region.set(null);
    this.regionError.set(null);
    this.phase.set('form');
  }

  typeLabel(value: string): string {
    return this.types.find((t) => t.value === value)?.label ?? 'Tipo não informado';
  }

  servicesFound(r: RegionResponse): number {
    return r.region.services.categories.reduce((n, c) => n + (c.services?.length ?? 0), 0);
  }

  profileGap(r: RegionResponse): string {
    const first = r.region.profile.unavailable.at(0);
    return first?.message ?? 'Sem dados para esta localização.';
  }

  /** Só manda o que foi preenchido. */
  private payload(): ManualPropertyInput {
    const raw = this.form.getRawValue();
    const out: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(raw)) {
      if (value === null || value === undefined) continue;
      if (typeof value === 'string') {
        const t = value.trim();
        if (t) out[key] = key === 'state' ? t.toUpperCase() : t;
      } else {
        out[key] = value;
      }
    }
    return out as unknown as ManualPropertyInput;
  }

  private handleError(err: HttpErrorResponse): void {
    if (err.status === 422) {
      const body = err.error as InvalidResponse;
      const mapped: Record<string, string> = {};
      const loose: string[] = [];
      for (const issue of body.errors ?? []) {
        const control = FIELD_MAP[issue.field] ?? (this.form.get(issue.field) ? issue.field : null);
        if (control) mapped[control] = issue.message;
        else loose.push(issue.message);
      }
      this.serverErrors.set(mapped);
      if (mapped['state'] || mapped['source_url']) this.showMore.set(true);
      this.generalError.set(loose.length ? loose.join(' ') : 'Revise os campos destacados.');
    } else if (err.status === 0) {
      this.generalError.set('Não foi possível falar com o CrabCrawler agora. Tente de novo em instantes.');
    } else {
      this.generalError.set((err.error as { error?: string })?.error ?? 'Algo deu errado ao salvar.');
    }
  }

  private loadRegion(id: string): void {
    this.properties.region(id).subscribe({
      next: (r) => this.region.set(r),
      error: () => this.regionError.set('A análise da região não respondeu agora.'),
    });
  }

  issues(list: FieldIssue[]): string[] {
    return list.map((i) => i.message);
  }
}
