use serde::Deserialize;
use std::fs;

use crate::simulator::TempRange;

#[derive(Deserialize, Debug)]
pub struct UncheckedTempRange {
    pub lower_threshold: f32,
    pub upper_threshold: f32,
}

#[derive(Deserialize, Debug)]
pub struct ConfigFile(pub Vec<ConfigEntry>);

impl ConfigFile {
    pub fn from_file(file_path: &str) -> Self {
        let contents = fs::read_to_string(file_path)
            .unwrap_or_else(|_| panic!("Could not read file `{}`", file_path));

        serde_json::from_str(&contents).expect("Could not deserialize config file")
    }
}

#[derive(Deserialize, Debug)]
pub struct ConfigEntry {
    pub start_time: u64,
    pub researcher: String,

    #[serde(default = "ConfigEntry::default_num_sensors")]
    pub num_sensors: usize,

    #[serde(default = "ConfigEntry::default_sample_rate")]
    pub sample_rate: u64,

    #[serde(default = "ConfigEntry::default_temp_range")]
    pub temp_range: TempRange,

    #[serde(default = "ConfigEntry::default_stabilization_samples")]
    pub stabilization_samples: u16,

    #[serde(default = "ConfigEntry::default_carry_out_samples")]
    pub carry_out_samples: u16,

    #[serde(default = "ConfigEntry::default_start_temperature")]
    pub start_temperature: f32,

    #[serde(skip)]
    pub secret_key: String,

    #[serde(skip)]
    pub topic: String,

    #[serde(skip)]
    pub topic_document: Option<String>,
}

impl ConfigEntry {
    fn default_num_sensors() -> usize {
        2
    }

    fn default_sample_rate() -> u64 {
        100
    }

    fn default_temp_range() -> TempRange {
        TempRange::new(25.5, 26.5).expect("Misconfigured default")
    }

    fn default_stabilization_samples() -> u16 {
        2
    }

    fn default_carry_out_samples() -> u16 {
        20
    }

    fn default_start_temperature() -> f32 {
        0.0
    }

    pub fn set_secret_key(&mut self, secret_key: &str) {
        self.secret_key = secret_key.into();
    }

    pub fn set_topic(&mut self, topic: &str) {
        self.topic = topic.into();
    }

    pub fn set_topic_document(&mut self, topic_document: Option<&str>) {
        self.topic_document = topic_document.map(|topic| topic.into());
    }
}
