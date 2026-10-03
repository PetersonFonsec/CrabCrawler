use chrono::{DateTime, Utc};
use crab_domain::PriceObservation;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PriceDirection {
    Down,
    Up,
}

/// Última mudança de preço de um anúncio, pronta para a interface.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceInsight {
    pub current_price_brl: f64,
    pub previous_price_brl: f64,
    /// Variação percentual (negativa = redução).
    pub change_pct: f64,
    pub direction: PriceDirection,
    pub changed_at: DateTime<Utc>,
    pub days_since_change: i64,
    /// Ex.: "Preço reduzido em 4,8% há 17 dias".
    pub message: String,
}

/// Compara as duas últimas observações de preço. `None` enquanto houver só
/// um preço conhecido. `history` pode vir em qualquer ordem.
pub fn price_insight(history: &[PriceObservation], now: DateTime<Utc>) -> Option<PriceInsight> {
    let mut sorted: Vec<&PriceObservation> = history.iter().collect();
    sorted.sort_by_key(|o| o.observed_at);
    let [.., previous, current] = sorted.as_slice() else {
        return None;
    };
    if previous.price_brl <= 0.0 || previous.price_brl == current.price_brl {
        return None;
    }
    let change_pct = (current.price_brl - previous.price_brl) / previous.price_brl * 100.0;
    let direction = if change_pct < 0.0 {
        PriceDirection::Down
    } else {
        PriceDirection::Up
    };
    let days = (now - current.observed_at).num_days().max(0);
    let verb = match direction {
        PriceDirection::Down => "reduzido",
        PriceDirection::Up => "aumentado",
    };
    let when = match days {
        0 => "hoje".to_string(),
        1 => "há 1 dia".to_string(),
        n => format!("há {n} dias"),
    };
    let pct = format!("{:.1}", change_pct.abs()).replace('.', ",");
    Some(PriceInsight {
        current_price_brl: current.price_brl,
        previous_price_brl: previous.price_brl,
        change_pct,
        direction,
        changed_at: current.observed_at,
        days_since_change: days,
        message: format!("Preço {verb} em {pct}% {when}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use uuid::Uuid;

    fn obs(price: f64, days_ago: i64, now: DateTime<Utc>) -> PriceObservation {
        PriceObservation {
            listing_id: Uuid::nil(),
            price_brl: price,
            observed_at: now - Duration::days(days_ago),
        }
    }

    #[test]
    fn first_price_has_no_insight() {
        let now = Utc::now();
        assert_eq!(price_insight(&[obs(620_000.0, 30, now)], now), None);
        assert_eq!(price_insight(&[], now), None);
    }

    #[test]
    fn reduction_message() {
        let now = Utc::now();
        let h = [obs(590_000.0, 17, now), obs(620_000.0, 40, now)];
        let i = price_insight(&h, now).unwrap();
        assert_eq!(i.direction, PriceDirection::Down);
        assert_eq!(i.message, "Preço reduzido em 4,8% há 17 dias");
        assert_eq!(i.previous_price_brl, 620_000.0);
    }

    #[test]
    fn increase_message() {
        let now = Utc::now();
        let h = [obs(500_000.0, 10, now), obs(550_000.0, 0, now)];
        let i = price_insight(&h, now).unwrap();
        assert_eq!(i.direction, PriceDirection::Up);
        assert_eq!(i.message, "Preço aumentado em 10,0% hoje");
    }
}
