use std::{thread, time::Duration};

fn main() {
    let milliseconds = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "100".into())
        .parse()
        .expect("milliseconds");
    thread::sleep(Duration::from_millis(milliseconds));
}
