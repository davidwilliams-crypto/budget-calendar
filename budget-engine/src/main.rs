use std::io::{self, BufRead};
fn main() {
    // Raw lines: a line that is not UTF-8 gets the WASM `request` answer instead of ending the run.
    for line in io::stdin().lock().split(b'\n') {
        let Ok(mut line) = line else { break };
        if line.last() == Some(&b'\r') { line.pop(); }
        match String::from_utf8(line) {
            Ok(line) => println!("{}", budget_engine::request_json(&line)),
            Err(_) => println!("{{\"ok\":false,\"error\":\"Input must be UTF-8.\"}}"),
        }
    }
}
