use std::time::Instant;

#[derive(Debug)]
pub struct Timed<T> {
    pub value: T,
    pub timestamp: Instant,
}

impl<T> Timed<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            timestamp: Instant::now(),
        }
    }
}


