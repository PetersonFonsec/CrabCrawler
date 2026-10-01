use crab_domain::Listing;
use serde::Serialize;

/// Comparação do preço/m² de um imóvel com imóveis semelhantes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceComparison {
    pub price_per_m2: f64,
    pub median_price_per_m2: f64,
    /// Diferença percentual: negativo = mais barato que a mediana.
    pub diff_pct: f64,
    pub sample_size: usize,
}

/// Compara o preço/m² de `listing` com a mediana de `comparables` do mesmo
/// tipo, transação e bairro. Retorna `None` se faltarem dados.
pub fn compare_price(listing: &Listing, comparables: &[Listing]) -> Option<PriceComparison> {
    let own = listing.price_per_m2()?;
    let values: Vec<f64> = comparables
        .iter()
        .filter(|c| c.id != listing.id)
        .filter(|c| c.transaction == listing.transaction && c.kind == listing.kind)
        .filter(|c| c.location.neighborhood_slug == listing.location.neighborhood_slug)
        .filter_map(Listing::price_per_m2)
        .collect();
    compare_price_per_m2(own, values)
}

/// Compara um preço/m² com a mediana de outros valores já filtrados.
pub fn compare_price_per_m2(own: f64, mut values: Vec<f64>) -> Option<PriceComparison> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let mid = values.len() / 2;
    let median = if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    };
    Some(PriceComparison {
        price_per_m2: own,
        median_price_per_m2: median,
        diff_pct: (own - median) / median * 100.0,
        sample_size: values.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::{normalize_listing, Gazetteer};
    use crab_domain::{DataSource, ListingKind, RawListing, TransactionType};

    fn listing(id: &str, price: f64, area: f64) -> Listing {
        let raw = RawListing {
            external_id: id.into(),
            url: None,
            title: id.into(),
            transaction: TransactionType::Sale,
            kind: ListingKind::Apartment,
            price_brl: Some(price),
            area_m2: Some(area),
            bedrooms: None,
            bathrooms: None,
            parking_spots: None,
            location_text: "São Bernardo do Campo - SP / Rudge Ramos".into(),
            postal_code: None,
            lat: None,
            lon: None,
        };
        normalize_listing(raw, DataSource::ListingFixture, &Gazetteer::mvp())
    }

    #[test]
    fn compares_against_median() {
        let target = listing("a", 440_000.0, 80.0); // 5500/m²
        let others = vec![
            listing("b", 500_000.0, 80.0), // 6250
            listing("c", 480_000.0, 80.0), // 6000
            listing("d", 520_000.0, 80.0), // 6500
        ];
        let cmp = compare_price(&target, &others).unwrap();
        assert_eq!(cmp.median_price_per_m2, 6250.0);
        assert_eq!(cmp.sample_size, 3);
        assert!((cmp.diff_pct - (-12.0)).abs() < 1e-9);
    }
}
