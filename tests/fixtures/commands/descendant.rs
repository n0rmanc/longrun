use std::{process::Command, thread, time::Duration};

fn main() {
    if std::env::args().any(|argument| argument == "--child") {
        thread::sleep(Duration::from_secs(60));
        return;
    }

    Command::new(std::env::current_exe().expect("current executable"))
        .arg("--child")
        .spawn()
        .expect("spawn child");
    thread::sleep(Duration::from_secs(60));
}
