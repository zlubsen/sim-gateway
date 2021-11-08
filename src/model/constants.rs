use std::time::Duration;

// general
pub const COMMAND_CHANNEL_CAPACITY : usize = 10;
pub const DATA_CHANNEL_CAPACITY : usize = 10;
pub const STATS_CHANNEL_CAPACITY : usize = 10;

// input
pub const INPUT_POLL_RATE_MS : u64 = 100;

// runtime
pub const BROADCAST_TTL : u32 = 1; // only broadcast on the local subnet
pub const STATS_AGGREGATION_WINDOW_SECS: u64 = 2; // interval at which statistics are aggregated
pub const STATS_PRINTER_RATE_MS: Duration = Duration::from_millis(5000); // interval at which statistics are published via stdout

// gui
pub const RENDER_RATE_MS : Duration = Duration::from_millis(1000);

// config - defaults
pub const DEFAULT_BUFFER_SIZE_BYTES: usize = 32768;
pub const DEFAULT_MAX_CONNECTIONS : usize = 1;
pub const DEFAULT_TTL : u32 = 64; // default for unix and mac
pub const DEFAULT_BLOCK_HOST : bool = true;
pub const DEFAULT_ENABLED : bool = true;
