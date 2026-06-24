//! Parse a chord-notation string to its pitch-class set, via the vibe-grammars
//! pest parser (micahscopes/vibe-grammars, pinned). vibe-grammars is a pure
//! syntax parser (root note + quality token + extensions) with no music theory,
//! so the pitch classes are computed here from the parse. This is the music
//! analogue of parsing Solidity source: a real string in, structure out.

use pest::Parser as _;
use vibe_grammars::elements::{Parser, Rule};

/// Parse one chord string ("Cmaj7", "Am7", "G7", "F#dim", or solfege like
/// "Cdo") into its pitch-class set (each 0..=11, unique, ascending). Errors
/// unless the whole trimmed string is a single chord.
pub fn chord_to_pitch_classes(s: &str) -> Result<Vec<i32>, String> {
    let input = s.trim();
    if input.is_empty() {
        return Err("empty chord string".to_string());
    }
    let mut pairs = Parser::parse(Rule::chord, input).map_err(|e| format!("parse error: {e}"))?;
    let chord = pairs.next().ok_or_else(|| "no chord parsed".to_string())?;
    // pest can match a prefix; require the whole input to be one chord.
    if chord.as_str() != input {
        return Err(format!(
            "not a single chord: {input:?} (matched only {:?})",
            chord.as_str()
        ));
    }

    let mut root: Option<i32> = None;
    let mut quality = String::new();
    let mut exts: Vec<(i32, u32)> = Vec::new();
    for inner in chord.into_inner() {
        match inner.as_rule() {
            Rule::note => root = Some(note_pc(inner.as_str())?),
            Rule::quality => quality = inner.as_str().to_string(),
            Rule::extension => {
                if let Some(e) = parse_ext(inner.as_str()) {
                    exts.push(e);
                }
            }
            Rule::parenthetical_extension => {
                let t = inner.as_str().trim_matches(|c| c == '(' || c == ')');
                if let Some(e) = parse_ext(t) {
                    exts.push(e);
                }
            }
            _ => {}
        }
    }
    let root = root.ok_or_else(|| "no root note".to_string())?;

    let (third, fifth) = triad(&quality);
    let mut intervals = vec![0, third, fifth];
    for (acc, deg) in &exts {
        if let Some(iv) = degree_interval(*deg, &quality) {
            intervals.push(iv + acc);
        }
    }
    let mut set: Vec<i32> = intervals
        .into_iter()
        .map(|iv| (root + iv).rem_euclid(12))
        .collect();
    set.sort_unstable();
    set.dedup();
    Ok(set)
}

fn note_pc(text: &str) -> Result<i32, String> {
    let mut chars = text.chars();
    let base = chars.next().ok_or_else(|| "empty note".to_string())?;
    let mut pc: i32 = match base {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return Err(format!("bad note base {base:?}")),
    };
    for c in chars {
        match c {
            '#' | '\u{266f}' => pc += 1,
            'b' | '\u{266d}' => pc -= 1,
            '\u{266e}' => {}
            _ => {}
        }
    }
    Ok(pc.rem_euclid(12))
}

fn is_minor(q: &str) -> bool {
    matches!(q, "m" | "min" | "minor" | "-")
}
fn is_dim(q: &str) -> bool {
    q.starts_with("dim") || q == "o" || q == "\u{00b0}" || q == "\u{00f8}"
}
fn is_aug(q: &str) -> bool {
    q.starts_with("aug") || q == "+"
}
fn is_major_seventh(q: &str) -> bool {
    matches!(q, "maj" | "major" | "ma" | "M" | "\u{0394}" | "\u{25b3}" | "j")
}

/// Triad (third, fifth) intervals above the root for a quality token.
fn triad(q: &str) -> (i32, i32) {
    if is_dim(q) {
        (3, 6)
    } else if is_aug(q) {
        (4, 8)
    } else if is_minor(q) {
        (3, 7)
    } else {
        (4, 7)
    }
}

/// Interval (semitones above the root) contributed by a chord degree; the
/// seventh depends on the quality.
fn degree_interval(deg: u32, q: &str) -> Option<i32> {
    Some(match deg {
        2 => 2,
        4 => 5,
        5 => 7,
        6 => 9,
        7 => {
            if is_major_seventh(q) {
                11
            } else if is_dim(q) {
                9
            } else {
                10
            }
        }
        9 => 2,
        11 => 5,
        13 => 9,
        _ => return None,
    })
}

/// "7", "b5", "#11", "sus4", "add9", "13" -> (accidental offset, degree number).
fn parse_ext(t: &str) -> Option<(i32, u32)> {
    let t = t.trim();
    let (acc, rest) = if let Some(r) = t.strip_prefix('b').or_else(|| t.strip_prefix('\u{266d}')) {
        (-1, r)
    } else if let Some(r) = t.strip_prefix('#').or_else(|| t.strip_prefix('\u{266f}')) {
        (1, r)
    } else {
        (0, t)
    };
    let rest = rest.trim_start_matches(|c: char| c.is_alphabetic());
    rest.parse::<u32>().ok().map(|d| (acc, d))
}

#[cfg(test)]
mod tests {
    use super::chord_to_pitch_classes as pcs;

    fn set(v: &[i32]) -> Vec<i32> {
        let mut v: Vec<i32> = v.to_vec();
        v.sort_unstable();
        v.dedup();
        v
    }

    #[test]
    fn common_chords_map_to_their_note_sets() {
        assert_eq!(pcs("C").unwrap(), set(&[0, 4, 7]));
        assert_eq!(pcs("Cmaj7").unwrap(), set(&[0, 4, 7, 11]));
        assert_eq!(pcs("Am7").unwrap(), set(&[9, 0, 4, 7]));
        assert_eq!(pcs("G7").unwrap(), set(&[7, 11, 2, 5]));
        assert_eq!(pcs("Dm").unwrap(), set(&[2, 5, 9]));
        assert_eq!(pcs("Cdim").unwrap(), set(&[0, 3, 6]));
    }

    #[test]
    fn solfege_quality_reads_as_a_plain_triad() {
        // the grammar parses "Cdo" as note C + quality "do"; treat as a triad
        assert_eq!(pcs("Cdo").unwrap(), set(&[0, 4, 7]));
    }

    #[test]
    fn non_chords_are_rejected() {
        assert!(pcs("").is_err());
        assert!(pcs("not a chord at all").is_err());
    }
}
