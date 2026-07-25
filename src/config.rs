use {
    cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry},
    serde::{Deserialize, Serialize},
};

#[derive(Debug, Default, Deserialize, Serialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Bits,
    #[default]
    Bytes,
}

#[derive(Debug, Deserialize, Serialize, Clone, CosmicConfigEntry, PartialEq)]
#[version = 1]
pub struct BitrateAppletConfig {
    pub unit: Unit,
    pub update_rate: f32,
    pub show_download_speed: bool,
    pub show_upload_speed: bool,
}

impl Default for BitrateAppletConfig {
    fn default() -> Self {
        BitrateAppletConfig {
            unit: Unit::Bytes,
            update_rate: 1.0,
            show_download_speed: true,
            show_upload_speed: true,
        }
    }
}
