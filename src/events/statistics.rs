#[derive(Debug, Clone)]
pub enum Statistics {
    TotalBytes(usize, u64, u64),
    AverageBytes(usize, f64, f64),
}
