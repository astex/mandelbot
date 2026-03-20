pub struct Config {
    pub shell: String,
}

impl Config {
    pub fn new() -> Self {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        Self { shell }
    }
}
