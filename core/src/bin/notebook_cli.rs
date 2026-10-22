//! Developer inspection harness, not the production H.I. Private handwriting tests
//! remain deferred to the complete app. All input uses the versioned host protocol.
use std::io::{self, BufRead, Write};
use stem_core::host::Host;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: notebook_cli ABSOLUTE_DATABASE_PATH")?;
    if !std::path::Path::new(&path).is_absolute() {
        return Err("database path must be absolute".into());
    }
    let host = Host::open(path.into(), 32)?;
    let mut stdout = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line?;
        let response = match host.request(&line) {
            Ok(response) => serde_json::to_string(&response)?,
            Err(error) => {
                serde_json::json!({"version":1,"ok":false,"error":error.to_string()}).to_string()
            }
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}
