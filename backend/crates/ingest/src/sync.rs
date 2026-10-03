//! Sincronização em lote por provider/parceiro, com registro da execução.

use std::collections::HashSet;
use std::time::Instant;

use chrono::Utc;
use crab_crawler::PropertySourceProvider;
use crab_domain::{PropertySourcePartner, SyncItemError, SyncMetrics, SyncRunStatus};
use crab_persistence::{SyncRunFinish, WriteOutcome};
use serde::Serialize;
use serde_json::json;

use crate::{IngestError, PropertyIngestor};

/// Quantos erros por item ficam gravados na execução (o total está nas
/// métricas; o log tem todos).
pub const MAX_STORED_ITEM_ERRORS: usize = 200;

#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Trava contra feed truncado: se a fração de anúncios ativos que
    /// sumiria passar disso, nada é inativado.
    pub max_deactivation_ratio: f64,
    /// A trava só vale a partir deste número de anúncios ativos.
    pub guard_min_active: i64,
    /// Ignora a trava (decisão consciente do operador).
    pub allow_mass_deactivation: bool,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            max_deactivation_ratio: 0.5,
            guard_min_active: 10,
            allow_mass_deactivation: false,
        }
    }
}

/// O que foi decidido sobre anúncios ausentes.
#[derive(Debug, Clone, Serialize)]
pub struct DeactivationDecision {
    pub applied: bool,
    pub missing: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub run_id: i64,
    pub provider: String,
    pub partner: Option<String>,
    pub status: SyncRunStatus,
    pub metrics: SyncMetrics,
    pub duration_ms: i64,
    pub deactivation: Option<DeactivationDecision>,
    pub item_errors: Vec<SyncItemError>,
}

impl PropertyIngestor {
    /// Busca o lote do provider, grava item a item (um item inválido não
    /// interrompe o lote), inativa ausentes quando o lote é completo e
    /// registra tudo em `property_sync_runs`.
    pub async fn sync(
        &self,
        provider: &dyn PropertySourceProvider,
        partner: Option<&PropertySourcePartner>,
        options: &SyncOptions,
    ) -> Result<SyncReport, IngestError> {
        let source = provider.source();
        let partner_id = partner.map(|p| p.id);
        let partner_slug = partner.map(|p| p.slug.clone());
        let now = Utc::now();
        let clock = Instant::now();
        let run_id = self.repo.start_run(source, partner_id, now).await?;
        tracing::info!(run_id, provider = source.as_str(), partner = ?partner_slug, "sincronização iniciada");

        let batch = match provider.fetch().await {
            Ok(b) => b,
            Err(e) => {
                let message = e.to_string();
                let duration_ms = clock.elapsed().as_millis() as i64;
                self.repo
                    .finish_run(
                        run_id,
                        &SyncRunFinish {
                            status: SyncRunStatus::Failed,
                            metrics: SyncMetrics::default(),
                            duration_ms,
                            error: Some(&message),
                            item_errors: &[],
                            details: json!({}),
                        },
                    )
                    .await?;
                tracing::error!(run_id, provider = source.as_str(), partner = ?partner_slug, error = %message, duration_ms, "sincronização falhou");
                return Ok(SyncReport {
                    run_id,
                    provider: source.as_str().into(),
                    partner: partner_slug,
                    status: SyncRunStatus::Failed,
                    metrics: SyncMetrics::default(),
                    duration_ms,
                    deactivation: None,
                    item_errors: vec![SyncItemError {
                        external_id: None,
                        stage: "fetch".into(),
                        reason: message,
                    }],
                });
            }
        };

        let mut metrics = SyncMetrics::default();
        let mut errors: Vec<SyncItemError> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut unreadable: Vec<String> = Vec::new();
        let mut valid = 0u32;

        for item in batch.items {
            metrics.received += 1;
            let mut raw = match item {
                Ok(raw) => raw,
                Err(e) => {
                    metrics.invalid += 1;
                    if let Some(id) = &e.external_id {
                        if seen.insert(id.clone()) {
                            unreadable.push(id.clone());
                        }
                    }
                    errors.push(SyncItemError {
                        external_id: e.external_id,
                        stage: "parse".into(),
                        reason: e.reason,
                    });
                    continue;
                }
            };
            raw.source = Some(source);
            raw.partner_id = partner_id;
            if let Some(id) = &raw.external_id {
                if !seen.insert(id.trim().to_string()) {
                    metrics.invalid += 1;
                    errors.push(SyncItemError {
                        external_id: Some(id.clone()),
                        stage: "parse".into(),
                        reason: "id repetido no mesmo lote; só a primeira ocorrência vale".into(),
                    });
                    continue;
                }
            }
            let external_id = raw.external_id.clone();
            match self.ingest(raw, now).await {
                Ok(outcome) => {
                    valid += 1;
                    metrics.created += outcome.count(WriteOutcome::Created);
                    metrics.updated += outcome.count(WriteOutcome::Updated);
                    metrics.unchanged += outcome.count(WriteOutcome::Unchanged);
                }
                Err(IngestError::Invalid(f)) => {
                    metrics.invalid += 1;
                    if let Some(id) = &external_id {
                        unreadable.push(id.trim().to_string());
                    }
                    errors.push(SyncItemError {
                        external_id,
                        stage: "normalize".into(),
                        reason: f.to_string(),
                    });
                }
                Err(IngestError::Database(e)) => {
                    metrics.failed += 1;
                    tracing::error!(run_id, external_id = ?external_id, error = %e, "falha ao gravar item");
                    errors.push(SyncItemError {
                        external_id,
                        stage: "persist".into(),
                        reason: "falha ao gravar (detalhe no log)".into(),
                    });
                }
            }
        }

        if !unreadable.is_empty() {
            self.repo
                .mark_unreadable(source, partner_id, &unreadable, now)
                .await?;
        }

        let deactivation = if batch.complete {
            let missing = self.repo.count_missing(source, partner_id, now).await?;
            let active = self.repo.count_active(source, partner_id).await?;
            let ratio = if active > 0 {
                missing as f64 / active as f64
            } else {
                0.0
            };
            let decision = if valid == 0 {
                DeactivationDecision {
                    applied: false,
                    missing,
                    reason: "nenhum anúncio válido no lote; inativação suspensa".into(),
                }
            } else if !options.allow_mass_deactivation
                && active >= options.guard_min_active
                && ratio > options.max_deactivation_ratio
            {
                DeactivationDecision {
                    applied: false,
                    missing,
                    reason: format!(
                        "{missing} de {active} anúncios sumiram (acima de {:.0}%); inativação suspensa até confirmação",
                        options.max_deactivation_ratio * 100.0
                    ),
                }
            } else {
                let done = self
                    .repo
                    .deactivate_missing(source, partner_id, now, Utc::now())
                    .await?;
                metrics.deactivated = done as u32;
                DeactivationDecision {
                    applied: true,
                    missing,
                    reason: "anúncios ausentes do lote completo marcados como INACTIVE".into(),
                }
            };
            Some(decision)
        } else {
            None
        };

        let duration_ms = clock.elapsed().as_millis() as i64;
        for e in &errors {
            tracing::warn!(run_id, external_id = ?e.external_id, stage = %e.stage, reason = %e.reason, "item recusado");
        }
        self.repo
            .finish_run(
                run_id,
                &SyncRunFinish {
                    status: SyncRunStatus::Succeeded,
                    metrics,
                    duration_ms,
                    error: None,
                    item_errors: &errors[..errors.len().min(MAX_STORED_ITEM_ERRORS)],
                    details: json!({ "source": batch.metadata, "deactivation": deactivation }),
                },
            )
            .await?;
        tracing::info!(
            run_id,
            provider = source.as_str(),
            partner = ?partner_slug,
            received = metrics.received,
            created = metrics.created,
            updated = metrics.updated,
            unchanged = metrics.unchanged,
            invalid = metrics.invalid,
            deactivated = metrics.deactivated,
            failed = metrics.failed,
            duration_ms,
            "sincronização concluída"
        );
        Ok(SyncReport {
            run_id,
            provider: source.as_str().into(),
            partner: partner_slug,
            status: SyncRunStatus::Succeeded,
            metrics,
            duration_ms,
            deactivation,
            item_errors: errors,
        })
    }
}
