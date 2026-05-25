use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quality {
    #[serde(rename = "128k")]
    Q128k,
    #[serde(rename = "192k")]
    Q192k,
    #[serde(rename = "320k")]
    Q320k,
    #[serde(rename = "flac")]
    Flac,
    #[serde(rename = "flac24bit")]
    Flac24bit,
    #[serde(rename = "ape")]
    Ape,
    #[serde(rename = "wav")]
    Wav,
}

impl Quality {
    pub fn as_str(&self) -> &'static str {
        match self {
            Quality::Q128k => "128k",
            Quality::Q192k => "192k",
            Quality::Q320k => "320k",
            Quality::Flac => "flac",
            Quality::Flac24bit => "flac24bit",
            Quality::Ape => "ape",
            Quality::Wav => "wav",
        }
    }
}

impl fmt::Display for Quality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Quality {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "128k" => Ok(Quality::Q128k),
            "192k" => Ok(Quality::Q192k),
            "320k" => Ok(Quality::Q320k),
            "flac" => Ok(Quality::Flac),
            "flac24bit" => Ok(Quality::Flac24bit),
            "ape" => Ok(Quality::Ape),
            "wav" => Ok(Quality::Wav),
            _ => Err(format!("Invalid quality: {}", s)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    Kuwo,
    NetEase,
    Kugou,
    QQ,
    Migu,
    Custom(String),
}

impl Source {
    pub fn as_str(&self) -> &str {
        match self {
            Source::Kuwo => "kw",
            Source::NetEase => "wy",
            Source::Kugou => "kg",
            Source::QQ => "tx",
            Source::Migu => "mg",
            Source::Custom(s) => s.as_str(),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<String> for Source {
    fn from(s: String) -> Self {
        match s.as_str() {
            "kw" => Source::Kuwo,
            "wy" => Source::NetEase,
            "kg" => Source::Kugou,
            "tx" => Source::QQ,
            "mg" => Source::Migu,
            _ => Source::Custom(s),
        }
    }
}

impl From<&str> for Source {
    fn from(s: &str) -> Self {
        Source::from(s.to_string())
    }
}

impl From<Source> for String {
    fn from(src: Source) -> Self {
        src.as_str().to_string()
    }
}

// Custom Serde implementation for Source
impl Serialize for Source {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Source {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Source::from(s))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicInfo {
    pub songmid: String,
    pub name: String,
    pub singer: String,
    pub source: Source,
    pub album_name: Option<String>,
    pub album_id: Option<String>,
    pub interval: Option<String>,
    pub pic_url: Option<String>,
    pub hash: Option<String>,
    pub extra: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub page: usize,
    pub limit: usize,
    pub total: usize,
    pub list: Vec<MusicInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_serialization() {
        let q = Quality::Q320k;
        let serialized = serde_json::to_string(&q).unwrap();
        assert_eq!(serialized, "\"320k\"");

        let deserialized: Quality = serde_json::from_str("\"flac24bit\"").unwrap();
        assert_eq!(deserialized, Quality::Flac24bit);
    }

    #[test]
    fn test_source_serialization() {
        let s = Source::Kuwo;
        let serialized = serde_json::to_string(&s).unwrap();
        assert_eq!(serialized, "\"kw\"");

        let deserialized: Source = serde_json::from_str("\"sixyin_v1.2.1\"").unwrap();
        assert_eq!(deserialized, Source::Custom("sixyin_v1.2.1".to_string()));
    }
}
