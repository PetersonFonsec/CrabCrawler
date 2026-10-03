import { registerLocaleData } from '@angular/common';
import localePt from '@angular/common/locales/pt';
import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { ComponentFixture, TestBed } from '@angular/core/testing';

import { API_BASE_URL } from '../../core/api.config';
import { ManualCreated } from '../../core/properties.service';
import { AnalyzePropertyComponent } from './analyze-property.component';

registerLocaleData(localePt, 'pt-BR');

const API = 'http://api.test';

const created: ManualCreated = {
  property: {
    id: 'p1',
    property_type: 'apartment',
    area_m2: 70,
    bedrooms: 2,
    parking_spaces: 1,
    street: 'Rua Exemplo',
    street_number: '100',
    neighborhood: 'Centro',
    municipality: 'São Bernardo do Campo',
    municipality_ibge_code: '3548708',
    state: 'SP',
    postal_code: '09750000',
    lat: null,
    lon: null,
    coordinate_source: null,
    coordinate_precision: null,
  },
  listing: { id: 'l1', source: 'MANUAL', transaction: 'sale', status: 'ACTIVE', price_brl: 500000, source_url: null },
  warnings: [],
  notes: [],
  analysis: { region_ready: false, municipality_ready: true, comparison_ready: true, missing: ['Coordenada do imóvel'] },
};

describe('AnalyzePropertyComponent', () => {
  let fixture: ComponentFixture<AnalyzePropertyComponent>;
  let component: AnalyzePropertyComponent;
  let http: HttpTestingController;
  let el: HTMLElement;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [AnalyzePropertyComponent],
      providers: [provideHttpClient(), provideHttpClientTesting(), { provide: API_BASE_URL, useValue: API }],
    }).compileComponents();

    fixture = TestBed.createComponent(AnalyzePropertyComponent);
    component = fixture.componentInstance;
    http = TestBed.inject(HttpTestingController);
    el = fixture.nativeElement;
    fixture.detectChanges();
  });

  afterEach(() => http.verify());

  it('não envia sem preço e localização', () => {
    component.submit();
    fixture.detectChanges();
    http.expectNone(`${API}/properties/manual`);
    expect(component.error('price')).toBe('Campo obrigatório.');
    expect(component.locationMissing()).toBeTrue();
    expect(component.showMore()).toBeTrue();
  });

  it('aceita cidade + UF no lugar do CEP', () => {
    component.form.patchValue({ price: 300000, municipality: 'Santo André', state: 'sp' });
    component.submit();
    const req = http.expectOne(`${API}/properties/manual`);
    expect(req.request.body).toEqual({ transaction: 'sale', price: 300000, municipality: 'Santo André', state: 'SP' });
    req.flush(created);
    http.expectOne(`${API}/properties/p1/region`).flush({}, { status: 500, statusText: 'x' });
  });

  it('envia só os campos preenchidos e mostra o resultado', () => {
    component.form.patchValue({ price: 500000, postal_code: '09750-000', bedrooms: 2, street: '  ' });
    component.submit();
    const req = http.expectOne(`${API}/properties/manual`);
    expect(req.request.method).toBe('POST');
    expect(req.request.body).toEqual({ transaction: 'sale', price: 500000, postal_code: '09750-000', bedrooms: 2 });
    req.flush(created);
    fixture.detectChanges();

    expect(component.phase()).toBe('done');
    expect(el.textContent).toContain('Apartamento');
    expect(el.textContent).toContain('Coordenada do imóvel');
    expect(el.querySelectorAll('.chips li.is-ok').length).toBe(2);

    http.expectOne(`${API}/properties/p1/region`).flush({}, { status: 500, statusText: 'x' });
    fixture.detectChanges();
    expect(el.textContent).toContain('A análise da região não respondeu agora.');
  });

  it('mostra erros de campo vindos da API (422)', () => {
    component.form.patchValue({ price: 500000, postal_code: '00000-000' });
    component.submit();
    http.expectOne(`${API}/properties/manual`).flush(
      { error: 'inválido', errors: [{ field: 'postal_code', message: 'CEP inválido' }], warnings: [] },
      { status: 422, statusText: 'Unprocessable Entity' },
    );
    fixture.detectChanges();
    expect(component.phase()).toBe('form');
    expect(component.error('postal_code')).toBe('CEP inválido');
    expect(el.textContent).toContain('Revise os campos destacados.');
  });

  it('avisa quando a API está fora do ar', () => {
    component.form.patchValue({ price: 500000, postal_code: '09750-000' });
    component.submit();
    http.expectOne(`${API}/properties/manual`).error(new ProgressEvent('error'));
    expect(component.generalError()).toContain('Não foi possível falar com o CrabCrawler');
  });
});
