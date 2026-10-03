use crab_domain::{ListingDetails, PropertyDetails};
use sha2::{Digest, Sha256};

/// Hash do conteúdo normalizado de um anúncio (imóvel + dados do anúncio).
///
/// Serve para detectar mudança entre sincronizações sem comparar campo a
/// campo. O payload bruto fica de fora: metadados voláteis da fonte (data de
/// publicação do feed, ordem das tags) não contam como mudança do imóvel.
pub fn content_hash(property: &PropertyDetails, listing: &ListingDetails) -> String {
    let canonical = serde_json::json!({ "property": property, "listing": listing });
    let digest = Sha256::digest(canonical.to_string().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crab_domain::TransactionType;

    fn listing(price: f64) -> ListingDetails {
        ListingDetails {
            transaction: TransactionType::Sale,
            price_brl: price,
            condominium_fee_brl: None,
            property_tax_brl: None,
            title: Some("Apartamento".into()),
            description: None,
            notes: None,
            source_url: None,
            images: vec![],
        }
    }

    #[test]
    fn hash_changes_only_with_content() {
        let p = PropertyDetails::default();
        assert_eq!(
            content_hash(&p, &listing(1.0)),
            content_hash(&p, &listing(1.0))
        );
        assert_ne!(
            content_hash(&p, &listing(1.0)),
            content_hash(&p, &listing(2.0))
        );
        assert_eq!(content_hash(&p, &listing(1.0)).len(), 64);
    }
}
