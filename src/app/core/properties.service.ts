import { HttpClient } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { API_BASE_URL } from './api.config';

export type Transaction = 'sale' | 'rent';

/** Corpo de `POST /properties/manual`. Só finalidade, preço e localização são obrigatórios. */
export interface ManualPropertyInput {
  transaction: Transaction;
  price: number;
  property_type?: string;
  postal_code?: string;
  street?: string;
  number?: string;
  complement?: string;
  neighborhood?: string;
  municipality?: string;
  state?: string;
  area_m2?: number;
  bedrooms?: number;
  bathrooms?: number;
  suites?: number;
  parking_spaces?: number;
  condominium_fee?: number;
  property_tax?: number;
  source_url?: string;
  notes?: string;
}

export interface FieldIssue {
  field: string;
  message: string;
}

export interface PropertyRow {
  id: string;
  property_type: string;
  area_m2: number | null;
  bedrooms: number | null;
  parking_spaces: number | null;
  street: string | null;
  street_number: string | null;
  neighborhood: string | null;
  municipality: string | null;
  municipality_ibge_code: string | null;
  state: string | null;
  postal_code: string | null;
  lat: number | null;
  lon: number | null;
  coordinate_source: string | null;
  coordinate_precision: string | null;
}

export interface ListingRow {
  id: string;
  source: string;
  transaction: Transaction;
  status: string;
  price_brl: number;
  source_url: string | null;
}

export interface AnalysisReadiness {
  region_ready: boolean;
  municipality_ready: boolean;
  comparison_ready: boolean;
  missing: string[];
}

export interface ManualCreated {
  property: PropertyRow;
  listing: ListingRow;
  warnings: FieldIssue[];
  notes: string[];
  analysis: AnalysisReadiness;
}

/** Resposta 422: problemas campo a campo. */
export interface InvalidResponse {
  error: string;
  errors: FieldIssue[];
  warnings: FieldIssue[];
}

export interface NoData {
  item: string;
  message: string;
}

export interface RegionResponse {
  region: {
    profile: { status: string; indicators: unknown[]; unavailable: NoData[] };
    services: { status: string; radius_m: number; categories: { services?: unknown[] }[] };
    environmental_risks: { assessment: string; message: string };
  };
}

@Injectable({ providedIn: 'root' })
export class PropertiesService {
  private readonly http = inject(HttpClient);
  private readonly api = inject(API_BASE_URL);

  createManual(input: ManualPropertyInput): Observable<ManualCreated> {
    return this.http.post<ManualCreated>(`${this.api}/properties/manual`, input);
  }

  region(propertyId: string): Observable<RegionResponse> {
    return this.http.get<RegionResponse>(`${this.api}/properties/${propertyId}/region`);
  }
}
