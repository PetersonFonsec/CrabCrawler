//! Modelo de domínio do CrabCrawler.
//!
//! Este crate não depende de I/O: só tipos e regras. Crawler, processamento,
//! persistência e API dependem dele, nunca o contrário.

pub mod indicator;
pub mod listing;
pub mod location;
pub mod property;
pub mod regional;
pub mod security;
pub mod source;

pub use indicator::{Indicator, IndicatorKind, RegionIndicator};
pub use listing::{Listing, ListingKind, PropertyType, RawListing, TransactionType};
pub use location::{GeoPoint, Location, Region, RegionLevel};
pub use property::{
    Address, CoordinatePrecision, CoordinateSource, Coordinates, ListingDetails, ListingStatus,
    ManualPropertyInput, PartnerStatus, PartnerType, PriceObservation, Property, PropertyDetails,
    PropertyListing, PropertySource, PropertySourcePartner, ProviderCapability, RawProperty,
    SyncItemError, SyncMetrics, SyncRunStatus,
};
pub use security::{
    CountingUnit, CrimeOccurrence, CrimeStatistic, CrimeType, Period, PeriodGranularity,
    RawCrimeRecord, SecurityProvenance,
};
pub use source::{DataSource, Provenance, SourceMetadata};
