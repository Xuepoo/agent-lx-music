#[cfg(feature = "kg")]
pub mod kg;
#[cfg(feature = "kw")]
pub mod kw;
#[cfg(feature = "mg")]
pub mod mg;
#[cfg(feature = "tx")]
pub mod tx;
#[cfg(feature = "wy")]
pub mod wy;

use lux_core::traits::MusicSource;
use lux_core::types::Source;
use std::sync::Arc;

pub fn get_native_source(source: &Source) -> Option<Arc<dyn MusicSource>> {
    match source {
        #[cfg(feature = "wy")]
        Source::NetEase => Some(Arc::new(wy::NetEaseSource)),
        #[cfg(feature = "kw")]
        Source::Kuwo => Some(Arc::new(kw::KuwoSource)),
        #[cfg(feature = "kg")]
        Source::Kugou => Some(Arc::new(kg::KugouSource)),
        #[cfg(feature = "tx")]
        Source::QQ => Some(Arc::new(tx::QQSource)),
        #[cfg(feature = "mg")]
        Source::Migu => Some(Arc::new(mg::MiguSource)),
        _ => None,
    }
}
