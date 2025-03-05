use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct General {
    pub debug: bool,
}

#[derive(Debug, Deserialize)]
pub struct Paths {
    pub log_dir: String,
    pub save_dir: String,
}

#[derive(Debug, Deserialize)]
pub struct Colors {
    pub background: String,
    pub foreground: String,
    pub highlight: String,
    pub focused: String,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub general: General,
    pub paths: Paths,
    pub colors: Colors,
}
