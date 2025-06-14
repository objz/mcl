use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct General {
    pub debug: bool,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub general: General,
}
