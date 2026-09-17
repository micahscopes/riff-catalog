//! Bounded JSONL transport. Product comparison semantics live in the library.
use std::io::{BufRead, Write};

use anyhow::{Result, bail};
use riff_catalog_region::protocol::{RESPONSE_SCHEMA, Request, compare};
use serde_json::json;

const MAX_LINE_BYTES: u64 = 1_048_576;

pub fn run() -> Result<()> {
    let errors = process(std::io::stdin().lock(), std::io::stdout().lock())?;
    if errors > 0 {
        bail!("{errors} region request(s) failed; see JSONL error records");
    }
    Ok(())
}

fn process(mut input: impl BufRead, mut output: impl Write) -> Result<usize> {
    use std::io::Read;
    let mut errors = 0;
    let mut line_number = 0;
    loop {
        let mut bytes = Vec::new();
        let count = input
            .by_ref()
            .take(MAX_LINE_BYTES + 1)
            .read_until(b'\n', &mut bytes)?;
        if count == 0 {
            break;
        }
        line_number += 1;
        let oversized = bytes.len() as u64 > MAX_LINE_BYTES;
        if oversized && bytes.last() != Some(&b'\n') {
            // Drain without accumulating an unbounded malicious record.
            loop {
                let chunk = input.fill_buf()?;
                if chunk.is_empty() {
                    break;
                }
                let end = chunk.iter().position(|&b| b == b'\n');
                let consumed = end.map_or(chunk.len(), |position| position + 1);
                input.consume(consumed);
                if end.is_some() {
                    break;
                }
            }
        }
        let mut request_id = None;
        let result = (|| -> Result<_> {
            if oversized {
                bail!("request exceeds 1 MiB line limit");
            }
            let request: Request = serde_json::from_slice(&bytes)?;
            request_id = Some(request.id.clone());
            Ok(serde_json::to_value(compare(request)?)?)
        })();
        let row = match result {
            Ok(row) => row,
            Err(error) => {
                errors += 1;
                json!({"schema": RESPONSE_SCHEMA, "id": request_id,
                    "line": line_number, "status": "error", "message": error.to_string()})
            }
        };
        serde_json::to_writer(&mut output, &row)?;
        writeln!(output)?;
        output.flush()?;
    }
    Ok(errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_records_follow_errors_and_keep_request_ids() {
        let region = json!({"operations":[{"op":"add", "operands":[
            {"External":"x"},{"Literal":"1"}]}], "outputs":[0]});
        let request = json!({"schema":"riffcat-ordered-region-request/1",
            "id":"ok", "view":"literal-tokens", "left":region, "right":region});
        let input = format!("bad json\n{request}\n");
        let mut output = Vec::new();
        assert_eq!(process(input.as_bytes(), &mut output).unwrap(), 1);
        let text = String::from_utf8(output).unwrap();
        let rows: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows[1]["id"], "ok");
        assert_eq!(rows[1]["equivalent"], true);
    }

    #[test]
    fn malformed_and_oversized_records_do_not_stop_the_batch() {
        let mut input = vec![b'x'; MAX_LINE_BYTES as usize + 2];
        input.extend_from_slice(b"\nnot json\n");
        let mut output = Vec::new();
        assert_eq!(process(&input[..], &mut output).unwrap(), 2);
        let rows: Vec<serde_json::Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["status"], "error");
        assert_eq!(rows[1]["line"], 2);
    }
}
