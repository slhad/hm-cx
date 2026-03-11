pub struct Config {
    pub port: u16,
}

impl Config {
    pub fn new(port: Option<u16>) -> Self {
        Config {
            port: port.unwrap_or(8080),
        }
    }
}
